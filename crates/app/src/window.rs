use crate::dbworker::{DbWorker, JobOut};
use crate::feedly_sync;
use crate::list;
use crate::model::{ListRow, UiState};
use crate::net::{Net, NetEvent};
use crate::reader::{find_in_view, find_next, ReaderPane};
use crate::sidebar;
use crate::state::*;
use crate::style::{gtk_css_for, tokens_for, ReaderStyleState};
use adw::prelude::*;
use webkit6::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::mpsc::Receiver;
use std::time::Duration;
use storage::{ArticleRow, Counts, FeedRow, GroupRow, Filter, Scope};

fn now_ms_stub() -> i64 {
    storage::now_ms()
}

pub fn dbg_log(msg: &str) {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var("LF_DEBUG").is_ok()) {
        eprintln!("[lf] {msg}");
    }
}

pub fn now_ms() -> i64 {
    storage::now_ms()
}

pub fn data_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home).join(".local/share")
        });
    base.join("lesefluss")
}

const JS_CAPTURE_POS: &str = "(()=>{const els=document.querySelectorAll('article.lf-body > *');if(!els.length)return '-1:0';const y=window.scrollY;let idx=0;for(let i=0;i<els.length;i++){const top=els[i].getBoundingClientRect().top+window.scrollY;if(top>y){idx=Math.max(0,i-1);break;}idx=i;}const el=els[idx];if(!el)return idx+':0';const off=y-(el.getBoundingClientRect().top+window.scrollY);return idx+':'+Math.round(off);})()";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectionCause {
    Unknown,
    Pointer,
    Keyboard,
}

type UndoBatch = Vec<(i64, String, bool, bool)>;
type PendingCb = Box<dyn FnOnce(&Rc<App>, JobOut)>;

const ACCENTS: &[&str] = &["#B94A2E", "#2E5FA3", "#3F8F5F", "#AC3A50", "#7A5CA8", "#B08A2E", "#2E8B8B"];

pub struct App {
    pub window: adw::ApplicationWindow,
    pub toast: adw::ToastOverlay,
    pub outer: adw::NavigationSplitView,
    pub inner: adw::NavigationSplitView,
    pub sidebar_list: gtk::ListBox,
    pub sidebar_title: adw::WindowTitle,
    pub sidebar_filters: RefCell<Vec<Option<Scope>>>,
    pub last_sync_label: gtk::Label,
    pub list_title: adw::WindowTitle,
    pub list_stack: gtk::Stack,
    pub list_store: gio::ListStore,
    pub list_selection: gtk::SingleSelection,
    pub list_view: gtk::ListView,
    pub list_scroll: gtk::ScrolledWindow,
    pub list_empty: adw::StatusPage,
    pub search_bar: gtk::SearchBar,
    pub search_entry: gtk::SearchEntry,
    pub filter_saved: gtk::ToggleButton,
    pub filter_unread: gtk::ToggleButton,
    pub filter_all: gtk::ToggleButton,
    pub reader: Rc<ReaderPane>,
    pub state: RefCell<UiState>,
    pub worker: DbWorker,
    pub net: Rc<Net>,
    pub media: std::sync::Arc<provider_local::media::MediaCache>,
    pub pending_db: RefCell<Vec<(Receiver<JobOut>, PendingCb)>>,
    pub pending_media: std::sync::Arc<std::sync::Mutex<Vec<(ArticleRow, String, Vec<(String, String)>)>>>,
    pub drain_active: Cell<bool>,
    pub preview_timer: RefCell<Option<glib::SourceId>>,
    pub search_timer: RefCell<Option<glib::SourceId>>,
    pub read_gen: Cell<u64>,
    pub load_gen: Cell<u64>,
    pub suppress: Cell<bool>,
    pub syncing_filters: Cell<bool>,
    pub selection_cause: Cell<SelectionCause>,
    pub undo_stack: RefCell<Vec<UndoBatch>>,
    pub tokens: RefCell<reader::tokens::Tokens>,
    pub css: gtk::CssProvider,
    pub panes: RefCell<Vec<gtk::Widget>>,
    me: RefCell<Option<Weak<App>>>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        application: &adw::Application,
        worker: DbWorker,
        net: Rc<Net>,
    ) -> Rc<Self> {
        let reader = Rc::new(ReaderPane::new());

        let sidebar_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(vec!["lf-sidebar".to_string(), "navigation-sidebar".to_string()])
            .build();
        let sidebar_scroll = gtk::ScrolledWindow::builder()
            .child(&sidebar_list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let last_sync_label = gtk::Label::builder()
            .label("Noch nicht aktualisiert")
            .xalign(0.0)
            .margin_start(16)
            .margin_end(16)
            .margin_top(6)
            .margin_bottom(10)
            .css_classes(vec!["lf-article-meta".to_string()])
            .build();
        let sidebar_footer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_footer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        sidebar_footer.append(&last_sync_label);

        let btn_hamburger = gtk::Button::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Quellen einblenden")
            .action_name("win.toggle-sources")
            .build();
        let btn_refresh = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Aktualisieren (Strg+R)")
            .action_name("win.refresh")
            .build();
        let btn_add = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Feed hinzufügen (Strg+N)")
            .action_name("win.add-feed")
            .build();
        let library_menu = gio::Menu::new();
        library_menu.append(Some("Feedly verbinden …"), Some("win.connect-feedly"));
        library_menu.append(Some("OPML importieren …"), Some("win.import-opml"));
        library_menu.append(Some("OPML exportieren …"), Some("win.export-opml"));
        library_menu.append(Some("Backup erstellen …"), Some("win.backup"));
        library_menu.append(Some("Aus Backup wiederherstellen …"), Some("win.restore"));
        let btn_library = gtk::MenuButton::builder()
            .icon_name("emblem-system-symbolic")
            .tooltip_text("Bibliothek: OPML, Backup")
            .menu_model(&library_menu)
            .build();
        let sidebar_title = adw::WindowTitle::new("Lesefluss", "Lokale Bibliothek");
        let sidebar_header = adw::HeaderBar::builder().title_widget(&sidebar_title).build();
        sidebar_header.pack_start(&btn_hamburger);
        sidebar_header.pack_end(&btn_library);
        sidebar_header.pack_end(&btn_refresh);
        sidebar_header.pack_end(&btn_add);

        let sidebar_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_body.append(&sidebar_scroll);
        sidebar_body.append(&sidebar_footer);
        sidebar_body.add_css_class("lf-sidebar");
        let sidebar_toolbar = adw::ToolbarView::builder().content(&sidebar_body).build();
        sidebar_toolbar.add_top_bar(&sidebar_header);
        let sources_page = adw::NavigationPage::new(&sidebar_toolbar, "Quellen");

        let list_store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let list_selection = gtk::SingleSelection::builder().model(&list_store).autoselect(false).build();
        let factory = gtk::SignalListItemFactory::new();
        let list_view = gtk::ListView::builder()
            .model(&list_selection)
            .factory(&factory)
            .single_click_activate(true)
            .css_classes(vec!["lf-articles".to_string(), "lf-list".to_string()])
            .build();
        let list_scroll = gtk::ScrolledWindow::builder()
            .css_classes(vec!["lf-list-bg".to_string()])
            .child(&list_view)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let list_empty = adw::StatusPage::builder()
            .icon_name("mailbox-symbolic")
            .title("Keine Artikel")
            .description("In dieser Ansicht ist gerade nichts los.")
            .vexpand(true)
            .build();
        let list_stack = gtk::Stack::builder().css_classes(vec!["lf-list-bg".to_string()]).build();
        list_stack.add_named(&list_scroll, Some("list"));
        list_stack.add_named(&list_empty, Some("empty"));
        list_stack.set_visible_child_name("list");

        let search_entry = gtk::SearchEntry::builder().placeholder_text("Artikel durchsuchen (Strg+L)").build();
        let search_bar = gtk::SearchBar::builder().child(&search_entry).show_close_button(true).build();

        let list_title = adw::WindowTitle::new("Ungelesen", "");
        let list_header = adw::HeaderBar::builder().title_widget(&list_title).build();
        let list_menu = gio::Menu::new();
        list_menu.append(Some("Bereich als gelesen markieren…"), Some("win.mark-scope-read"));
        let list_more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&list_menu)
            .tooltip_text("Listenaktionen")
            .build();
        list_header.pack_end(&list_more);
        let filter_saved = gtk::ToggleButton::builder()
            .icon_name("user-bookmarks-symbolic")
            .tooltip_text("Gespeicherte Artikel anzeigen")
            .build();
        let filter_unread = gtk::ToggleButton::builder()
            .icon_name("mail-unread-symbolic")
            .tooltip_text("Ungelesene Artikel anzeigen")
            .active(true)
            .build();
        let filter_all = gtk::ToggleButton::builder()
            .icon_name("view-list-symbolic")
            .tooltip_text("Alle Artikel anzeigen")
            .build();
        let filter_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        filter_box.add_css_class("lf-filterbar");
        filter_box.append(&filter_saved);
        filter_box.append(&filter_unread);
        filter_box.append(&filter_all);
        let filter_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        filter_wrap.set_halign(gtk::Align::Center);
        filter_wrap.set_margin_bottom(8);
        filter_wrap.append(&filter_box);
        let list_toolbar = adw::ToolbarView::builder().content(&list_stack).build();
        list_toolbar.add_top_bar(&list_header);
        list_toolbar.add_top_bar(&search_bar);
        list_toolbar.add_bottom_bar(&filter_wrap);
        let list_page = adw::NavigationPage::new(&list_toolbar, "Artikel");

        let btn_back = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Zurück zur Artikelliste")
            .action_name("win.reader-back")
            .build();
        reader.header.pack_start(&btn_back);

        let inner = adw::NavigationSplitView::builder()
            .min_sidebar_width(280.0)
            .max_sidebar_width(460.0)
            .sidebar(&list_page)
            .content(&adw::NavigationPage::new(&reader.toolbar, "Lesen"))
            .build();

        let outer = adw::NavigationSplitView::builder()
            .min_sidebar_width(208.0)
            .max_sidebar_width(320.0)
            .sidebar(&sources_page)
            .content(&adw::NavigationPage::new(&inner, "Artikel"))
            .build();

        btn_back.bind_property("visible", &inner, "collapsed").sync_create().build();
        btn_hamburger.bind_property("visible", &outer, "collapsed").sync_create().build();

        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&outer));
        let window = adw::ApplicationWindow::builder()
            .application(application)
            .default_width(1440)
            .default_height(900)
            .title("Lesefluss")
            .content(&toast)
            .build();
        window.add_css_class("lf-window");

        let css = gtk::CssProvider::new();
        let display = gtk::prelude::WidgetExt::display(&window);
        gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

        let app = Rc::new(Self {
            window,
            toast,
            outer,
            inner,
            sidebar_list,
            sidebar_title,
            sidebar_filters: RefCell::new(Vec::new()),
            last_sync_label,
            list_title,
            list_stack,
            list_store,
            list_selection,
            list_view,
            list_scroll,
            list_empty,
            search_bar,
            search_entry,
            filter_saved,
            filter_unread,
            filter_all,
            reader,
            state: RefCell::new(UiState::default()),
            worker,
            net,
            media: std::sync::Arc::new(
                provider_local::media::MediaCache::new(
                    provider_local::media::cache_dir(),
                    512 * 1024 * 1024,
                )
                .expect("Mediencache-Verzeichnis"),
            ),
            pending_db: RefCell::new(Vec::new()),
            pending_media: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            drain_active: Cell::new(false),
            preview_timer: RefCell::new(None),
            search_timer: RefCell::new(None),
            read_gen: Cell::new(0),
            load_gen: Cell::new(0),
            suppress: Cell::new(false),
            syncing_filters: Cell::new(false),
            selection_cause: Cell::new(SelectionCause::Unknown),
            undo_stack: RefCell::new(Vec::new()),
            tokens: RefCell::new(tokens_for(true)),
            css,
            panes: RefCell::new(Vec::new()),
            me: RefCell::new(None),
        });

        *app.me.borrow_mut() = Some(Rc::downgrade(&app));
        *app.panes.borrow_mut() = vec![
            app.sidebar_list.clone().upcast::<gtk::Widget>(),
            app.list_view.clone().upcast::<gtk::Widget>(),
            app.reader.webview.clone().upcast::<gtk::Widget>(),
        ];

        app.apply_theme_now();
        app.wire(factory);
        app.register_actions(application);
        app.install_width_watcher();
        app.start_drain_loop();
        app.window.present();
        app.bootstrap();
        app
    }

    fn weak(&self) -> Weak<App> {
        self.me.borrow().clone().expect("App-Selbstreferenz gesetzt")
    }

    // ── DB- und Net-Drain ──

    pub fn db_query<F, R, C>(&self, f: F, on: C)
    where
        F: FnOnce(&storage::Database) -> R + Send + 'static,
        R: Send + 'static,
        C: FnOnce(&Rc<App>, R) + 'static,
    {
        let rx = self.worker.send(f);
        self.pending_db.borrow_mut().push((
            rx,
            Box::new(move |app: &Rc<App>, out: JobOut| {
                if let Ok(v) = out.downcast::<R>() {
                    on(app, *v);
                }
            }),
        ));
    }

    fn start_drain_loop(&self) {
        let w = self.weak();
        glib::timeout_add_local(Duration::from_millis(120), move || {
            let Some(app) = w.upgrade() else { return glib::ControlFlow::Break };
            app.drain_once();
            glib::ControlFlow::Continue
        });
    }

    fn drain_once(&self) {
        if let Ok(mut q) = self.pending_media.lock() {
            let jobs: Vec<_> = q.drain(..).collect();
            drop(q);
            for (row, mut html, reps) in jobs {
                let current = self.reader.current.borrow().clone();
                if current.as_deref() != Some(row.id.as_str()) {
                    continue;
                }
                for (u, d) in &reps {
                    html = html.replace(u, d).replace(&u.replace('&', "&amp;"), d);
                }
                let doc = self.reader_doc(&row, &html);
                self.reader.load_html_doc(&doc);
            }
        }
        loop {
            let net_event = self.net.events.try_recv().ok();
            if let Some(ev) = net_event {
                self.handle_net_event(ev);
                continue;
            }
            break;
        }
        let mut ready: Vec<(PendingCb, JobOut)> = Vec::new();
        {
            let mut pending = self.pending_db.borrow_mut();
            let mut i = 0;
            while i < pending.len() {
                match pending[i].0.try_recv() {
                    Ok(out) => {
                        let (_, cb) = pending.remove(i);
                        ready.push((cb, out));
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => i += 1,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let (_, _cb) = pending.remove(i);
                    }
                }
            }
        }
        for (cb, out) in ready {
            let self_rc = self
                .me
                .borrow()
                .as_ref()
                .and_then(Weak::upgrade)
                .expect("App lebendig während Drain");
            cb(&self_rc, out);
        }
    }

    fn handle_net_event(&self, ev: NetEvent) {
        match ev {
            NetEvent::FetchStarted(_) => {}
            NetEvent::FetchNotModified(_) => self.touch_last_sync(),
            NetEvent::FetchDone { feed_id, added, title, .. } => {
                self.touch_last_sync();
                let label = title.unwrap_or_else(|| format!("Feed {feed_id}"));
                if added > 0 {
                    self.show_toast(&format!("{label}: {added} neue Artikel"));
                    let at_top = self.list_scroll.vadjustment().value() < 80.0;
                    self.reload_counts();
                    if at_top {
                        self.load_page(false);
                    }
                } else {
                    self.reload_counts();
                }
            }
            NetEvent::FetchFailed { message, .. } => {
                self.touch_last_sync();
                self.show_toast(&format!("Abruf fehlgeschlagen: {message}"));
            }
            NetEvent::DiscoveryDone { candidates, .. } => self.show_discovery_dialog(candidates),
            NetEvent::DiscoveryFailed { message, .. } => {
                self.show_toast(&format!("Kein Feed gefunden: {message}"));
            }
            NetEvent::FeedlySyncDone { added } => {
                self.reload_meta_keep();
                if added > 0 {
                    self.show_toast(&format!("Feedly: {added} neue Artikel"));
                }
            }
            NetEvent::FeedlySyncFailed { message } => {
                self.show_toast(&format!("Feedly-Sync fehlgeschlagen: {message}"));
            }
        }
    }

    fn touch_last_sync(&self) {
        let now = now_ms();
        self.state.borrow_mut().last_sync = Some(now);
        self.last_sync_label.set_label(&format!("Zuletzt aktualisiert: {}", fmt_time(now)));
        self.worker.send(move |db| db.set_last_sync("local", now));
    }

    // ── Start ──

    fn bootstrap(&self) {
        let seed = std::env::var("LF_SEED").is_ok();
        let w = self.weak();
        self.db_query(
            move |db| {
                db.ensure_local_account()?;
                if seed {
                    let n = db.list_feeds()?;
                    if n.is_empty() {
                        crate::seed::seed_fixtures(db)?;
                    }
                }
                let feeds = db.list_feeds()?;
                let groups = db.list_groups()?;
                let counts = db.counts()?;
                let last = db.last_sync("local")?;
                let accounts = db.list_accounts()?;
                Ok::<_, storage::StorageError>((feeds, groups, counts, last, accounts))
            },
            move |app, res: storage::Result<(Vec<FeedRow>, Vec<GroupRow>, Counts, Option<i64>, Vec<(String, String, String)>)>| {
                let Ok((feeds, groups, counts, last, accounts)) = res else { return };
                let has_feedly_account = accounts.iter().any(|(_, k, _)| k == "feedly");
                {
                    let mut st = app.state.borrow_mut();
                    st.feeds = feeds;
                    st.groups = groups;
                    st.counts = counts;
                    st.last_sync = last;
                    st.accounts = accounts;
                }
                app.start_feedly_scheduler();
                if std::env::var("LF_FEEDLY_CONNECT").is_ok() && !has_feedly_account {
                    if let Some(token) = feedly_sync::token_from_disk() {
                        app.start_feedly(token);
                    }
                }
                if let Some(ms) = last {
                    app.last_sync_label.set_label(&format!("Zuletzt aktualisiert: {}", fmt_time(ms)));
                }
                let w2 = app.worker.clone();
                let media = std::sync::Arc::clone(&app.media);
                app.db_query(
                    move |db| {
                        let pinned = db.pinned_media_urls()?;
                        let now = storage::now_ms();
                        let pruned = db.prune_old_read(now, 90)?;
                        Ok::<_, storage::StorageError>((pinned, pruned))
                    },
                    move |_app, res: storage::Result<(Vec<String>, usize)>| {
                        if let Ok((pinned, pruned)) = res {
                            if pruned > 0 {
                                dbg_log(&format!("Aufbewahrung: {pruned} alte gelesene Artikel bereinigt"));
                            }
                            let keys: std::collections::HashSet<String> =
                                pinned.iter().map(|u| provider_local::media::key_of(u)).collect();
                            media.prune(&keys);
                        }
                        let _ = w2;
                    },
                );
                app.refresh_sidebar();
                app.load_page(false);
                if let Ok(list) = std::env::var("LF_SUBSCRIBE") {
                    for url in list.split(',').filter(|u| !u.trim().is_empty()) {
                        let url = url.trim().to_string();
                        let title = url::Url::parse(&url)
                            .ok()
                            .and_then(|u| u.host_str().map(str::to_string))
                            .unwrap_or_else(|| url.clone());
                        app.subscribe(&url, &title);
                    }
                }
                let _ = w;
            },
        );
    }

    fn load_page(&self, append: bool) {
        let (scope, filter, cursor, search) = {
            let st = self.state.borrow();
            let cur = if append { st.cursor.clone() } else { None };
            (st.scope.clone(), st.filter, cur, st.search.clone())
        };
        if append && cursor.is_none() {
            return;
        }
        let cur = cursor.clone();
        self.load_gen.set(self.load_gen.get() + 1);
        let gen = self.load_gen.get();
        self.db_query(
            move |db| match &search {
                Some(q) if !q.is_empty() => db.search(q, 200),
                _ => db.query_articles(&scope, filter, cur.as_ref().map(|(ms, id)| (*ms, id.as_str())), 200),
            },
            move |app, res: storage::Result<Vec<ArticleRow>>| {
                if app.load_gen.get() != gen {
                    dbg_log(&format!("load_page: stale gen {gen} verworfen"));
                    return;
                }
                let Ok(rows) = res else { return };
                if let Some(first) = rows.first() {
                    dbg_log(&format!("load_page gen={gen} rows={} first={} unread={}", rows.len(), first.id, first.unread));
                } else {
                    dbg_log(&format!("load_page gen={gen} rows=0"));
                }
                let had_full_page = rows.len() >= 200;
                let keep_sel = app.state.borrow().selected.clone();
                {
                    let mut st = app.state.borrow_mut();
                    if append {
                        st.append_rows(rows);
                    } else {
                        st.build_rows(rows);
                    }
                    if !had_full_page {
                        st.cursor = st.last_item().map(|a| (a.published_ms, a.id.clone()));
                    }
                    if let Some(sel) = keep_sel {
                        st.selected = Some(sel);
                    }
                }
                app.sync_store(append);
                app.update_list_empty_state();
                app.state.borrow_mut().loading_more = false;
                if let Some(sel) = app.selected_id() {
                    let current = app.reader.current.borrow().clone();
                    if current.as_deref() != Some(sel.as_str()) {
                        app.open_article_by_id(&sel, false, false);
                    }
                }
            },
        );
    }

    fn reload_counts(&self) {
        let w = self.weak();
        self.db_query(
            |db| db.counts(),
            move |app, res: storage::Result<Counts>| {
                let Ok(counts) = res else { return };
                app.state.borrow_mut().counts = counts;
                app.refresh_sidebar();
                app.update_reader_empty();
                let _ = w;
            },
        );
    }

    // ── Store-Sync ──

    fn sync_store(&self, append: bool) {
        let rows = self.state.borrow().rows.clone();
        self.suppress.set(true);
        if append {
            let existing = self.list_store.n_items() as usize;
            for r in rows.iter().skip(existing) {
                self.list_store.append(&glib::BoxedAnyObject::new(r.clone()));
            }
        } else {
            self.list_store.remove_all();
            for r in &rows {
                self.list_store.append(&glib::BoxedAnyObject::new(r.clone()));
            }
            if let Some(sel) = self.state.borrow().selected.clone() {
                if let Some(pos) = self.state.borrow().row_pos(&sel.1) {
                    self.list_selection.set_selected(pos as u32);
                }
            }
        }
        let has_items = rows.iter().any(|r| matches!(r, ListRow::Item(_)));
        self.list_stack.set_visible_child_name(if has_items { "list" } else { "empty" });
        self.suppress.set(false);
    }

    fn update_list_empty_state(&self) {
        let has = self.state.borrow().rows.iter().any(|r| matches!(r, ListRow::Item(_)));
        self.list_stack.set_visible_child_name(if has { "list" } else { "empty" });
    }

    fn rebind_row(&self, id: &str) {
        let Some(pos) = self.state.borrow().row_pos(id) else { return };
        let Some(obj) = self.list_store.item(pos as u32) else { return };
        self.suppress.set(true);
        self.list_store.remove(pos as u32);
        self.list_store.insert(pos as u32, &obj);
        self.suppress.set(false);
    }

    fn remove_row(&self, id: &str) {
        let Some(pos) = self.state.borrow().row_pos(id) else { return };
        {
            let mut st = self.state.borrow_mut();
            st.rows.remove(pos);
        }
        self.suppress.set(true);
        if self.selected_id().as_deref() == Some(id) {
            let next = self
                .state
                .borrow()
                .rows
                .iter()
                .enumerate()
                .skip(pos)
                .find_map(|(i, r)| match r {
                    ListRow::Item(_) => Some(i),
                    _ => None,
                })
                .or_else(|| {
                    self.state.borrow().rows.iter().enumerate().take(pos).rev().find_map(|(i, r)| match r {
                        ListRow::Item(_) => Some(i),
                        _ => None,
                    })
                });
            if let Some(p) = next {
                self.list_selection.set_selected(p as u32);
            }
        }
        self.list_store.remove(pos as u32);
        self.suppress.set(false);
        self.update_list_empty_state();
    }

    fn selected_id(&self) -> Option<String> {
        self.state.borrow().selected.clone().map(|(_, id)| id)
    }

    // ── Sidebar ──

    fn account_label_for_scope(&self) -> String {
        let st = self.state.borrow();
        let acc_name = |id: &str| -> String {
            st.accounts
                .iter()
                .find(|(a, _, _)| a == id)
                .map(|(_, k, n)| format!("{} · {}", k, n))
                .unwrap_or_else(|| "Konto".into())
        };
        match &st.scope {
            Scope::Global => {
                if st.accounts.iter().any(|(_, k, _)| k != "local") {
                    "Alle Konten".into()
                } else {
                    "Lokale Bibliothek".into()
                }
            }
            Scope::Account(a) => acc_name(a),
            Scope::Feed(f) => match st.feeds.iter().find(|x| x.id == *f) {
                Some(feed) if feed.account_id == "local" => "Lokale Bibliothek".into(),
                Some(feed) => acc_name(&feed.account_id),
                None => "Lokale Bibliothek".into(),
            },
            Scope::Group(g) => match st.groups.iter().find(|x| x.id == *g) {
                Some(gr) if gr.account_id == "local" => "Lokale Bibliothek".into(),
                Some(gr) => acc_name(&gr.account_id),
                None => "Lokale Bibliothek".into(),
            },
        }
    }

    fn refresh_sidebar(&self) {
        self.sidebar_title.set_subtitle(&self.account_label_for_scope());
        let state = self.state.borrow();
        let mut filters = self.sidebar_filters.borrow_mut();
        self.suppress.set(true);
        let w = self.weak();
        let cb: Rc<dyn Fn(i64)> = Rc::new(move |gid: i64| {
            if let Some(app) = w.upgrade() {
                app.toggle_group(gid);
            }
        });
        sidebar::rebuild(&self.sidebar_list, &state, &mut filters, &cb);
        self.suppress.set(false);
    }

    fn toggle_group(&self, gid: i64) {
        {
            let mut st = self.state.borrow_mut();
            if !st.collapsed.remove(&gid) {
                st.collapsed.insert(gid);
            }
        }
        self.refresh_sidebar();
    }

    fn set_scope(&self, f: Scope, reset_filter_to_unread: bool) {
        {
            let mut st = self.state.borrow_mut();
            st.scope = f.clone();
            if reset_filter_to_unread {
                st.filter = Filter::Unread;
            }
            st.search = None;
            st.selected = st.last_opened.get(&(f.clone(), st.filter)).cloned();
        }
        self.sync_filter_buttons();
        self.search_bar.set_search_mode(false);
        self.list_title.set_title(&self.scope_label_now());
        self.refresh_sidebar();
        self.load_page(false);
        if let Some(sel) = self.selected_id() {
            self.open_article_by_id(&sel, false, false);
        } else {
            self.update_reader_empty();
        }
        if self.outer.is_collapsed() {
            self.outer.set_show_content(true);
        }
    }

    fn set_filter(&self, f: Filter) {
        {
            let mut st = self.state.borrow_mut();
            st.filter = f;
            st.search = None;
            st.selected = st.last_opened.get(&(st.scope.clone(), f)).cloned();
        }
        self.sync_filter_buttons();
        self.load_page(false);
        if let Some(sel) = self.selected_id() {
            self.open_article_by_id(&sel, false, false);
        } else {
            self.update_reader_empty();
        }
    }

    fn sync_filter_buttons(&self) {
        let f = self.state.borrow().filter;
        self.syncing_filters.set(true);
        self.filter_saved.set_active(f == Filter::Saved);
        self.filter_unread.set_active(f == Filter::Unread);
        self.filter_all.set_active(f == Filter::All);
        self.syncing_filters.set(false);
    }

    fn scope_label_now(&self) -> String {
        let st = self.state.borrow();
        if let Some(q) = &st.search {
            return format!("Suche: {q}");
        }
        match &st.scope {
            Scope::Global => "Ungelesen".into(),
            Scope::Account(a) => st
                .accounts
                .iter()
                .find(|(id, _, _)| id == a)
                .map(|(_, _, n)| n.clone())
                .unwrap_or_else(|| "Konto".into()),
            Scope::Group(g) => st.groups.iter().find(|x| &x.id == g).map(|x| x.name.clone()).unwrap_or_else(|| "Gruppe".into()),
            Scope::Feed(f) => st.feed_title(*f),
        }
    }

    // ── Artikel öffnen / Status ──

    fn open_article_by_id(&self, id: &str, focus: bool, flush: bool) {
        let Some(row) = self.state.borrow().article(id) else { return };
        self.open_article(row, focus, flush);
    }

    fn open_article(&self, row: ArticleRow, focus: bool, _flush: bool) {
        let id = row.id.clone();
        {
            let mut st = self.state.borrow_mut();
            let key = (st.scope.clone(), st.filter);
            st.selected = Some((row.feed_id, id.clone()));
            st.last_opened.insert(key, (row.feed_id, id.clone()));
            st.unread_guard.remove(&id);
        }
        if let Some(pos) = self.state.borrow().row_pos(&id) {
            self.suppress.set(true);
            self.list_selection.set_selected(pos as u32);
            self.suppress.set(false);
        }

        self.reader.title.set_title(&row.title);
        self.reader.title.set_subtitle(&row.feed_title);
        self.update_reader_buttons(&row);
        let prev = self.reader.current.borrow().clone();
        if let Some(prev) = prev {
            if prev != id {
                self.capture_position(&prev);
            }
        }
        *self.reader.current.borrow_mut() = Some(id.clone());
        self.reader.show_loading();

        let row2 = row.clone();
        let id_cb = id.clone();
        let w = self.weak();
        self.db_query(
            move |db| db.content_html(row2.feed_id, &row2.id),
            move |app, res: storage::Result<Option<String>>| {
                let current = app.reader.current.borrow().clone();
                if current.as_deref() != Some(&id_cb) {
                    return;
                }
                match res {
                    Ok(Some(html)) => app.load_reader_html(row, html),
                    _ => app.reader.show_error(),
                }
                let _ = w;
            },
        );

        self.start_read_timer(id.clone());

        if self.inner.is_collapsed() {
            self.inner.set_show_content(true);
        }
        if focus {
            self.reader.webview.grab_focus();
        }
    }

    fn start_read_timer(&self, id: String) {
        self.read_gen.set(self.read_gen.get() + 1);
        let gen = self.read_gen.get();
        let w = self.weak();
        glib::timeout_add_local(Duration::from_millis(800), move || {
            let Some(app) = w.upgrade() else {
                dbg_log("read-timer: app tot");
                return glib::ControlFlow::Break;
            };
            if app.read_gen.get() != gen {
                dbg_log(&format!("read-timer {id}: gen ueberholt"));
                return glib::ControlFlow::Break;
            }
            if !app.window.is_active() {
                dbg_log(&format!("read-timer {id}: fenster inaktiv"));
                return glib::ControlFlow::Break;
            }
            if app.reader.current.borrow().as_deref() != Some(id.as_str()) {
                dbg_log(&format!("read-timer {id}: nicht mehr aktuell"));
                return glib::ControlFlow::Break;
            }
            if app.state.borrow().unread_guard.contains(&id) {
                dbg_log(&format!("read-timer {id}: guard"));
                return glib::ControlFlow::Break;
            }
            let still_unread = app.state.borrow().article(&id).map(|a| a.unread).unwrap_or(false);
            dbg_log(&format!("read-timer {id}: feuert, unread={still_unread}"));
            if still_unread {
                let mut batch: UndoBatch = Vec::new();
                app.apply_status(&id, Some(true), None, &mut batch);
                app.undo_stack.borrow_mut().push(batch);
            }
            glib::ControlFlow::Break
        });
    }

    fn apply_status(&self, id: &str, read: Option<bool>, saved: Option<bool>, batch: &mut UndoBatch) {
        let prev = self.state.borrow().article(id);
        let Some(mut cur) = prev else { return };
        let feed_id = cur.feed_id;
        let (prev_unread, prev_saved) = (cur.unread, cur.saved);
        batch.push((feed_id, id.to_string(), prev_unread, prev_saved));
        {
            if let Some(r) = read {
                cur.unread = !r;
            }
            if let Some(sv) = saved {
                cur.saved = sv;
            }
            self.state.borrow().set_article(id, cur);
        }
        {
            let mut st = self.state.borrow_mut();
            if let Some(r) = read {
                let delta = if r { -1 } else { 1 };
                st.counts.unread = (st.counts.unread + delta).max(0);
                if let Some(entry) = st.counts.per_feed.iter_mut().find(|(f, _)| *f == feed_id) {
                    entry.1 = (entry.1 + delta).max(0);
                }
                let gids: Vec<i64> = st
                    .feeds
                    .iter()
                    .find(|f| f.id == feed_id)
                    .map(|f| f.groups.clone())
                    .unwrap_or_default();
                for entry in st.counts.per_group.iter_mut() {
                    if gids.contains(&entry.0) {
                        entry.1 = (entry.1 + delta).max(0);
                    }
                }
            }
            if let Some(s) = saved {
                st.counts.saved = (st.counts.saved + if s { 1 } else { -1 }).max(0);
            }
        }

        let _ = feed_id;

        if self.reader.current.borrow().as_deref() == Some(id) {
            if let Some(row) = self.state.borrow().article(id) {
                self.update_reader_buttons(&row);
            }
        }
        self.refresh_sidebar();

        let feed_id2 = feed_id;
        let id2 = id.to_string();
        dbg_log(&format!("set_status job: {id2} read={read:?} saved={saved:?}"));
        self.worker.send(move |db| db.set_status(feed_id2, &id2, read, saved));
    }

    fn current_article(&self) -> Option<ArticleRow> {
        let id = self.selected_id().or_else(|| self.reader.current.borrow().clone())?;
        self.state.borrow().article(&id)
    }

    fn toggle_read(&self) {
        let Some(row) = self.current_article() else { return };
        let was_unread = row.unread;
        let mut batch: UndoBatch = Vec::new();
        self.apply_status(&row.id, Some(!was_unread), None, &mut batch);
        if was_unread {
            self.state.borrow_mut().unread_guard.remove(&row.id);
        } else {
            self.state.borrow_mut().unread_guard.insert(row.id.clone());
        }
        self.undo_stack.borrow_mut().push(batch);
    }

    fn toggle_saved(&self) {
        let Some(row) = self.current_article() else { return };
        let mut batch: UndoBatch = Vec::new();
        self.apply_status(&row.id, None, Some(!row.saved), &mut batch);
        self.undo_stack.borrow_mut().push(batch);
    }

    fn undo(&self) {
        let Some(batch) = self.undo_stack.borrow_mut().pop() else {
            self.show_toast("Nichts rückgängig zu machen");
            return;
        };
        {
            let st = self.state.borrow();
            for (feed_id, id, unread, saved) in &batch {
                if let Some(mut a) = st.article(id) {
                    a.unread = *unread;
                    a.saved = *saved;
                    st.set_article(id, a);
                }
                let _ = feed_id;
            }
        }
        let batch2 = batch.clone();
        self.worker.send(move |db| {
            for (feed_id, id, unread, saved) in &batch2 {
                db.set_status(*feed_id, id, Some(!unread), Some(*saved))?;
            }
            Ok::<_, storage::StorageError>(())
        });
        self.reload_counts();
        self.show_toast("Aktion rückgängig gemacht");
    }

    fn mark_scope_dialog(&self) {
        let ids: Vec<(i64, String)> = {
            let st = self.state.borrow();
            st.rows
                .iter()
                .filter_map(|r| {
                    let a = r.article()?;
                    if a.unread {
                        Some((a.feed_id, a.id.clone()))
                    } else {
                        None
                    }
                })
                .collect()
        };
        let label = self.scope_label_now();
        if ids.is_empty() {
            self.show_toast(&format!("„{label}“ enthält keine ungelesenen Artikel"));
            return;
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Bereich als gelesen markieren")
            .body(format!(
                "„{label}“: {} zum Klickzeitpunkt bekannte Artikel werden als gelesen markiert. Rückgängig mit Strg+Z.",
                ids.len()
            ))
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("mark", &format!("{} als gelesen markieren", ids.len()));
        dialog.set_response_appearance("mark", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp != "mark" {
                return;
            }
            let mut batch: UndoBatch = Vec::new();
            for (feed_id, id) in &ids {
                let _ = feed_id;
                app.apply_status(id, Some(true), None, &mut batch);
            }
            app.undo_stack.borrow_mut().push(batch);
            app.show_toast(&format!("{} Artikel als gelesen markiert", ids.len()));
        });
    }

    fn show_toast(&self, msg: &str) {
        self.toast.add_toast(adw::Toast::new(msg));
    }

    // ── Navigation ──

    fn move_selection(&self, delta: i32) {
        let positions: Vec<usize> = self
            .state
            .borrow()
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match r {
                ListRow::Item(_) => Some(i),
                _ => None,
            })
            .collect();
        if positions.is_empty() {
            return;
        }
        let cur_id = self.selected_id();
        let cur_idx = cur_id
            .as_ref()
            .and_then(|id| self.state.borrow().row_pos(id))
            .and_then(|p| positions.iter().position(|x| *x == p))
            .map(|i| i as i32)
            .unwrap_or(if delta > 0 { -1 } else { positions.len() as i32 });
        let target = (cur_idx + delta).clamp(0, positions.len() as i32 - 1);
        let idx = positions[target as usize];
        let row = self.state.borrow().rows.get(idx).and_then(|r| r.article());
        let Some(row) = row else { return };
        if self.selected_id().as_deref() == Some(row.id.as_str()) {
            return;
        }
        dbg_log(&format!("move_selection delta={delta} cur_idx={cur_idx} target={target} idx={idx} von {}", positions.len()));
        self.open_article(row, false, true);
        self.scroll_to_selected();
    }

    fn scroll_to_selected(&self) {
        if let Some(id) = self.selected_id() {
            if let Some(pos) = self.state.borrow().row_pos(&id) {
                dbg_log(&format!("scroll_to pos={pos}"));
                self.list_view.scroll_to(pos as u32, gtk::ListScrollFlags::NONE, None::<gtk::ScrollInfo>);
            }
        }
    }

    fn move_unread(&self, dir: i32) {
        let positions: Vec<usize> = self
            .state
            .borrow()
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let a = r.article()?;
                if a.unread {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        if positions.is_empty() {
            self.show_toast("Keine weiteren ungelesenen Artikel in dieser Ansicht");
            return;
        }
        let cur = self.selected_id().and_then(|id| self.state.borrow().row_pos(&id)).unwrap_or(0);
        let next = if dir > 0 {
            positions.iter().find(|&&p| p > cur).copied().or_else(|| positions.first().copied())
        } else {
            positions.iter().rev().find(|&&p| p < cur).copied().or_else(|| positions.last().copied())
        };
        if let Some(idx) = next {
            let row = self.state.borrow().rows.get(idx).and_then(|r| r.article());
            if let Some(row) = row {
                self.open_article(row, false, true);
                self.scroll_to_selected();
            }
        }
    }

    fn focus_pane(&self, dir: i32) {
        let panes = self.panes.borrow().clone();
        let Some(focus) = gtk::prelude::GtkWindowExt::focus(&self.window) else {
            if let Some(p) = panes.first() {
                gtk::prelude::GtkWindowExt::set_focus(&self.window, Some(p));
            }
            return;
        };
        let mut current: Option<usize> = None;
        let mut cur = Some(focus.clone());
        while let Some(c) = cur {
            if let Some(i) = panes.iter().position(|p| *p == c) {
                current = Some(i);
                break;
            }
            cur = c.parent();
        }
        let idx = match current {
            Some(i) => ((i as i32 + dir).rem_euclid(panes.len() as i32)) as usize,
            None => 0,
        };
        if let Some(p) = panes.get(idx) {
            gtk::prelude::GtkWindowExt::set_focus(&self.window, Some(p));
        }
    }

    fn back(&self) {
        if self.inner.is_collapsed() && self.inner.shows_content() {
            self.inner.set_show_content(false);
        } else if self.outer.is_collapsed() && self.outer.shows_content() {
            self.outer.set_show_content(false);
        }
    }

    // ── Reader-Aktionen ──

    fn update_reader_buttons(&self, row: &ArticleRow) {
        self.reader.btn_read.set_icon_name(if row.unread { "mail-read-symbolic" } else { "mail-unread-symbolic" });
        self.reader.btn_read.set_tooltip_text(Some(if row.unread {
            "Als gelesen markieren (M)"
        } else {
            "Als ungelesen markieren (M)"
        }));
        self.reader.btn_saved.set_icon_name(if row.saved { "user-bookmarks-symbolic" } else { "bookmark-new-symbolic" });
        self.reader.btn_saved.set_tooltip_text(Some(if row.saved { "Entspeichern (S)" } else { "Speichern (S)" }));
    }

    fn update_reader_empty(&self) {
        let st = self.state.borrow();
        let label = self.scope_label_now();
        let unread = match &st.scope {
            Scope::Global => st.counts.unread,
            Scope::Account(a) => st.counts.per_account.iter().find(|(id, _)| id == a).map(|(_, c)| *c).unwrap_or(0),
            Scope::Feed(f) => st.feed_unread(*f),
            Scope::Group(g) => st.group_unread(*g),
        };
        let total = st.rows.iter().filter(|r| matches!(r, ListRow::Item(_))).count() as i64;
        drop(st);
        self.reader.show_empty(&label, &format!("{unread} ungelesen · {total} Artikel"));
        self.reader.title.set_title(&label);
        self.reader.title.set_subtitle("");
    }

    fn reader_doc(&self, row: &ArticleRow, html: &str) -> String {
        let style = self.reader.style.borrow();
        let rs = reader::ReaderStyle {
            font_size: style.font_size,
            measure_ch: style.measure_ch,
            line_height: style.line_height,
        };
        let tokens = *self.tokens.borrow();
        let published = fmt_full(row.published_ms);
        let doc = reader::ReaderDocument {
            kicker: &row.feed_title,
            title: &row.title,
            author: row.author.as_deref(),
            source: "",
            published: &published,
            content_html: html,
        };
        reader::render_document(&doc, &tokens, &rs)
    }

    fn load_reader_html(&self, row: ArticleRow, html: String) {
        let imgs: Vec<(String, String)> = {
            let srcs = reader::sanitize::image_sources(&html);
            if let Ok(sel) = scraper::Selector::parse("img[src]") {
                let doc = scraper::Html::parse_fragment(&html);
                srcs
                    .into_iter()
                    .take(25)
                    .map(|u| {
                        let alt = doc
                            .select(&sel)
                            .find_map(|el| {
                                if el.value().attr("src") == Some(u.as_str()) {
                                    Some(el.value().attr("alt").unwrap_or("Bild").to_string())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| "Bild".to_string());
                        (u, alt)
                    })
                    .collect()
            } else {
                srcs.into_iter().take(25).map(|u| (u, "Bild".to_string())).collect()
            }
        };
        if imgs.is_empty() {
            let doc = self.reader_doc(&row, &html);
            self.reader.load_html_doc(&doc);
            return;
        }
        let media = std::sync::Arc::clone(&self.media);
        let http = self.net.http();
        let queue = self.pending_media.clone();
        self.net.spawn(async move {
            let mut reps: Vec<(String, String)> = Vec::new();
            for (u, alt) in imgs {
                match media.get_or_fetch(&http, &u).await {
                    Some((bytes, mime)) => {
                        reps.push((u, provider_local::media::data_uri(&bytes, &mime)));
                    }
                    None => {
                        reps.push((u, provider_local::media::placeholder_data_uri(&alt)));
                    }
                }
            }
            if let Ok(mut q) = queue.lock() {
                q.push((row, html, reps));
            }
        });
    }

    fn capture_position(&self, id: &str) {
        let Some(row) = self.state.borrow().article(id) else { return };
        let w = self.weak();
        let id2 = id.to_string();
        let row_db = row.clone();
        self.db_query(
            move |db| db.content_hash(row_db.feed_id, &row_db.id),
            move |app, res: storage::Result<Option<String>>| {
                let hash = res.ok().flatten();
                let w2 = w.clone();
                let id3 = id2.clone();
                app.reader.webview.evaluate_javascript(
                    JS_CAPTURE_POS,
                    None,
                    None,
                    None::<&gio::Cancellable>,
                    move |res| {
                        let Ok(v) = res else { return };
                        let s = v.to_string();
                        let mut parts = s.split(':');
                        let idx: i64 = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                        let off: i64 = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                        if idx < 0 {
                            return;
                        }
                        if let Some(app) = w2.upgrade() {
                            if let Some(r) = app.state.borrow().article(&id3) {
                                let _ = app.worker.send(move |db| {
                                    db.save_read_position(r.feed_id, &r.id, hash.as_deref(), idx, off)
                                });
                            }
                        }
                    },
                );
                let _ = w;
            },
        );
    }

    fn after_load_finished(&self) {
        if self.reader.pending_scroll.get() >= 0.0 {
            self.reader.restore_scroll();
            return;
        }
        let Some(id) = self.reader.current.borrow().clone() else { return };
        let Some(row) = self.state.borrow().article(&id) else { return };
        let w = self.weak();
        self.db_query(
            move |db| {
                let pos = db.read_position(row.feed_id, &row.id)?;
                let hash = db.content_hash(row.feed_id, &row.id)?;
                Ok::<_, storage::StorageError>((pos, hash))
            },
            move |app, res: storage::Result<(Option<(Option<String>, i64, i64)>, Option<String>)>| {
                let Ok((pos, hash)) = res else { return };
                let Some((saved_hash, idx, off)) = pos else { return };
                if saved_hash.is_some() && saved_hash != hash {
                    return;
                }
                if idx == 0 && off == 0 {
                    return;
                }
                let js = format!(
                    "(()=>{{const els=document.querySelectorAll('article.lf-body > *');const el=els[{idx}];if(el){{window.scrollTo(0, el.getBoundingClientRect().top+window.scrollY+{off});}}}})()"
                );
                app.reader.webview.evaluate_javascript(&js, None, None, None::<&gio::Cancellable>, |_| {});
                let _ = w;
            },
        );
    }

    fn zoom(&self, delta: f64) {
        {
            let mut s = self.reader.style.borrow_mut();
            s.font_size = (s.font_size + delta).clamp(14.0, 32.0);
        }
        self.reload_current(true);
    }

    fn zoom_reset(&self) {
        self.reader.style.borrow_mut().font_size = ReaderStyleState::default().font_size;
        self.reload_current(true);
    }

    fn reload_current(&self, preserve: bool) {
        let Some(id) = self.reader.current.borrow().clone() else { return };
        let Some(row) = self.state.borrow().article(&id) else { return };
        let w = self.weak();
        let fid = row.feed_id;
        let rid = row.id.clone();
        self.db_query(
            move |db| db.content_html(fid, &rid),
            move |app, res: storage::Result<Option<String>>| {
                let Ok(Some(html)) = res else { return };
                if preserve {
                    let pane = Rc::clone(&app.reader);
                    let wv = pane.webview.clone();
                    let row2 = row.clone();
                    let w2 = w.clone();
                    wv.evaluate_javascript(
                        "window.scrollY",
                        None,
                        None,
                        None::<&gio::Cancellable>,
                        move |res| {
                            if let Ok(v) = res {
                                if v.is_number() {
                                    pane.pending_scroll.set(v.to_double());
                                }
                            }
                            if let Some(app) = w2.upgrade() {
                                app.load_reader_html(row2, html);
                            }
                        },
                    );
                } else {
                    app.load_reader_html(row, html);
                }
                let _ = w;
            },
        );
    }

    fn open_find(&self) {
        self.reader.search_bar.set_search_mode(true);
        self.reader.search_entry.grab_focus();
    }

    fn open_external(&self) {
        let Some(url) = self.current_article().and_then(|a| a.url) else { return };
        gtk::UriLauncher::new(&url).launch(None::<&gtk::Window>, None::<&gio::Cancellable>, |res| {
            if let Err(e) = res {
                eprintln!("Extern öffnen fehlgeschlagen: {e}");
            }
        });
    }

    fn copy_link(&self) {
        let Some(url) = self.current_article().and_then(|a| a.url) else { return };
        self.window.clipboard().set_text(&url);
        self.show_toast("Link kopiert");
    }

    // ── Konto ──

    fn do_refresh(&self) {
        let feeds: Vec<(i64, String)> = self.state.borrow().feeds.iter().map(|f| (f.id, f.feed_url.clone())).collect();
        if feeds.is_empty() {
            self.show_toast("Noch keine Feeds abonniert (Strg+N)");
            return;
        }
        for (feed_id, url) in feeds {
            self.net.fetch_feed(self.worker.clone(), feed_id, url, true);
        }
        self.show_toast(&format!("Aktualisiere {} Feeds…", self.state.borrow().feeds.len()));
    }

    fn connect_feedly_dialog(&self) {
        match feedly_sync::token_from_disk() {
            Some(token) => self.start_feedly(token),
            None => {
                let entry = gtk::Entry::builder()
                    .placeholder_text("Feedly Developer Token einfügen")
                    .visibility(false)
                    .build();
                let dialog = adw::AlertDialog::builder()
                    .heading("Feedly verbinden")
                    .body("Privater Testzugang: Token unter feedly.com/v3/auth/dev bzw. via PKCE-Flow erzeugen und hier einfügen. Speicherung lokal (chmod 600); Schlüsselbund-Integration folgt.")
                    .extra_child(&entry)
                    .build();
                dialog.add_response("cancel", "Abbrechen");
                dialog.add_response("ok", "Verbinden");
                dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
                dialog.set_default_response(Some("ok"));
                dialog.set_close_response("cancel");
                let w = self.weak();
                dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
                    let Some(app) = w.upgrade() else { return };
                    if resp != "ok" {
                        return;
                    }
                    let token = entry.text().trim().to_string();
                    if token.is_empty() {
                        return;
                    }
                    if feedly_sync::save_token(&token).is_ok() {
                        app.start_feedly(token);
                    }
                });
            }
        }
    }

    fn start_feedly(&self, token: String) {
        self.show_toast("Feedly: Erst-Sync gestartet …");
        feedly_sync::initial_sync(self.worker.clone(), &self.net, token);
    }

    fn start_feedly_scheduler(&self) {
        let has_feedly = self.state.borrow().accounts.iter().any(|(_, k, _)| k == "feedly");
        if !has_feedly {
            return;
        }
        {
            let mut st = self.state.borrow_mut();
            if st.next_feedly_sync == 0 {
                st.next_feedly_sync = now_ms() + 60_000;
            }
        }
        let w = self.weak();
        glib::timeout_add_local(Duration::from_secs(60), move || {
            let Some(app) = w.upgrade() else { return glib::ControlFlow::Break };
            let due = {
                let mut st = app.state.borrow_mut();
                if st.accounts.iter().any(|(_, k, _)| k == "feedly") && now_ms() >= st.next_feedly_sync {
                    st.next_feedly_sync = now_ms() + 15 * 60_000;
                    true
                } else {
                    false
                }
            };
            if due {
                if let Some(token) = feedly_sync::token_from_disk() {
                    let (account_id, last_sync) = {
                        let st = app.state.borrow();
                        let acc = st.accounts.iter().find(|(_, k, _)| k == "feedly").map(|(id, _, _)| id.clone());
                        (acc, st.last_sync_for_feedly())
                    };
                    if let Some(account_id) = account_id {
                        feedly_sync::delta_sync(app.worker.clone(), &app.net, token, account_id, last_sync);
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    fn import_opml_dialog(&self) {
        let dlg = gtk::FileDialog::builder().title("OPML-Datei wählen").build();
        let w = self.weak();
        glib::MainContext::default().spawn_local(async move {
            let Ok(file) = dlg.open_future(None::<&gtk::Window>).await else { return };
            let Some(path) = file.path() else { return };
            let bytes = match std::fs::read(&path) {
                Ok(b) if b.len() <= crate::opml::MAX_OPML_BYTES => b,
                Ok(_) => {
                    if let Some(app) = w.upgrade() {
                        app.show_toast("OPML-Datei zu groß (Limit 20 MiB)");
                    }
                    return;
                }
                Err(e) => {
                    if let Some(app) = w.upgrade() {
                        app.show_toast(&format!("Lesefehler: {e}"));
                    }
                    return;
                }
            };
            let Ok(xml) = String::from_utf8(bytes) else {
                if let Some(app) = w.upgrade() {
                    app.show_toast("OPML-Datei ist kein UTF-8");
                }
                return;
            };
            match crate::opml::parse_opml(&xml) {
                Ok(draft) => {
                    if let Some(app) = w.upgrade() {
                        app.show_opml_preview(draft);
                    }
                }
                Err(e) => {
                    if let Some(app) = w.upgrade() {
                        app.show_toast(&format!("OPML-Fehler: {e}"));
                    }
                }
            }
        });
    }

    fn show_opml_preview(&self, draft: crate::opml::OpmlDraft) {
        let known: std::collections::HashSet<String> =
            self.state.borrow().feeds.iter().map(|f| f.feed_url.clone()).collect();
        let new: Vec<&crate::opml::OpmlFeed> = draft.feeds.iter().filter(|f| !known.contains(&f.xml_url)).collect();
        let existing = draft.feeds.len() - new.len();
        let listing: String = new
            .iter()
            .take(40)
            .map(|f| format!("• {} — {}\n", f.title, f.xml_url))
            .collect();
        let body = format!(
            "{} neue Feeds, {} bestehende (bleiben erhalten, Gruppen werden zusammengeführt).{}",
            new.len(),
            existing,
            if draft.errors.is_empty() {
                String::new()
            } else {
                format!(" {} ungültige Einträge übersprungen.", draft.errors.len())
            }
        );
        let label = gtk::Label::builder()
            .label(&listing)
            .xalign(0.0)
            .wrap(true)
            .margin_start(12)
            .margin_end(12)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .child(&label)
            .max_content_height(280)
            .min_content_height(80)
            .build();
        let dialog = adw::AlertDialog::builder()
            .heading("OPML-Import")
            .body(body)
            .extra_child(&scroll)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("import", &format!("{} Feeds importieren", new.len()));
        dialog.set_response_appearance("import", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("import"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp == "import" {
                app.import_opml(draft.clone());
            }
        });
    }

    fn import_opml(&self, draft: crate::opml::OpmlDraft) {
        let w = self.weak();
        self.db_query(
            move |db| {
                db.ensure_local_account()?;
                let groups = db.list_groups()?;
                let mut group_ids: std::collections::HashMap<String, i64> =
                    groups.into_iter().map(|g| (g.name, g.id)).collect();
                let mut new_feeds: Vec<(i64, String)> = Vec::new();
                let mut merged = 0usize;
                for f in &draft.feeds {
                    let mut gids: Vec<i64> = Vec::new();
                    for gname in &f.groups {
                        let gid = match group_ids.get(gname) {
                            Some(g) => *g,
                            None => {
                                let g = db.add_group("local", gname, None)?;
                                group_ids.insert(gname.clone(), g);
                                g
                            }
                        };
                        gids.push(gid);
                    }
                    match db.feed_id_by_url(&f.xml_url)? {
                        Some(fid) => {
                            let existing = db.list_feeds()?.into_iter().find(|x| x.id == fid);
                            let mut all = existing.map(|x| x.groups).unwrap_or_default();
                            for g in gids {
                                if !all.contains(&g) {
                                    all.push(g);
                                }
                            }
                            db.set_feed_groups(fid, &all)?;
                            merged += 1;
                        }
                        None => {
                            let accent = crate::window::ACCENTS[f.xml_url.len() % crate::window::ACCENTS.len()];
                            let fid = db.add_feed("local", &f.xml_url, &f.title, f.html_url.as_deref(), accent)?;
                            db.set_feed_groups(fid, &gids)?;
                            new_feeds.push((fid, f.xml_url.clone()));
                        }
                    }
                }
                Ok::<_, storage::StorageError>((new_feeds, merged))
            },
            move |app, res: storage::Result<(Vec<(i64, String)>, usize)>| {
                let Ok((new_feeds, merged)) = res else { return };
                for (fid, url) in &new_feeds {
                    app.net.fetch_feed(app.worker.clone(), *fid, url.clone(), true);
                }
                app.reload_meta_keep();
                app.show_toast(&format!("{} Feeds importiert, {} zusammengeführt", new_feeds.len(), merged));
                let _ = w;
            },
        );
    }

    fn reload_meta_keep(&self) {
        let w = self.weak();
        self.db_query(
            |db| Ok::<_, storage::StorageError>((db.list_feeds()?, db.list_groups()?, db.counts()?)),
            move |app, res: storage::Result<(Vec<storage::FeedRow>, Vec<storage::GroupRow>, storage::Counts)>| {
                let Ok((feeds, groups, counts)) = res else { return };
                {
                    let mut st = app.state.borrow_mut();
                    st.feeds = feeds;
                    st.groups = groups;
                    st.counts = counts;
                }
                app.refresh_sidebar();
                app.load_page(false);
                let _ = w;
            },
        );
    }

    fn export_opml(&self) {
        let feeds: Vec<crate::opml::OpmlFeed> = {
            let st = self.state.borrow();
            st.feeds
                .iter()
                .map(|f| crate::opml::OpmlFeed {
                    title: f.title.clone(),
                    xml_url: f.feed_url.clone(),
                    html_url: None,
                    groups: f
                        .groups
                        .iter()
                        .filter_map(|g| st.groups.iter().find(|x| x.id == *g).map(|x| x.name.clone()))
                        .collect(),
                })
                .collect()
        };
        let dlg = gtk::FileDialog::builder().title("OPML-Export speichern unter").build();
        dlg.set_initial_name(Some("lesefluss-abonnements.opml"));
        let w = self.weak();
        glib::MainContext::default().spawn_local(async move {
            let Ok(file) = dlg.save_future(None::<&gtk::Window>).await else { return };
            let Some(path) = file.path() else { return };
            let xml = crate::opml::build_opml(&feeds);
            let tmp = path.with_extension("opml.tmp");
            let ok = std::fs::write(&tmp, xml.as_bytes()).is_ok() && std::fs::rename(&tmp, &path).is_ok();
            if let Some(app) = w.upgrade() {
                app.show_toast(if ok { "OPML exportiert" } else { "Export fehlgeschlagen" });
            }
        });
    }

    fn backup_dialog(&self) {
        let dlg = gtk::FileDialog::builder().title("Backup speichern unter").build();
        dlg.set_initial_name(Some("lesefluss-backup.db"));
        let w = self.weak();
        let worker = self.worker.clone();
        glib::MainContext::default().spawn_local(async move {
            let Ok(file) = dlg.save_future(None::<&gtk::Window>).await else { return };
            let Some(path) = file.path() else { return };
            let res = worker.send(move |db| db.backup_to(&path)).recv();
            let ok = res.ok().and_then(|b| b.downcast_ref::<storage::Result<()>>().map(|r| r.is_ok())).unwrap_or(false);
            if let Some(app) = w.upgrade() {
                app.show_toast(if ok { "Backup erstellt" } else { "Backup fehlgeschlagen" });
            }
        });
    }

    fn restore_dialog(&self) {
        let dlg = gtk::FileDialog::builder().title("Backup-Datei wählen").build();
        let w = self.weak();
        glib::MainContext::default().spawn_local(async move {
            let Ok(file) = dlg.open_future(None::<&gtk::Window>).await else { return };
            let Some(path) = file.path() else { return };
            let pending = data_dir().join("restore.pending");
            let ok = std::fs::copy(&path, &pending).is_ok();
            if let Some(app) = w.upgrade() {
                app.show_toast(if ok {
                    "Backup vorgemerkt — wird beim nächsten Start wiederhergestellt"
                } else {
                    "Wiederherstellung fehlgeschlagen"
                });
            }
        });
    }

    fn add_feed_dialog(&self) {
        let entry = gtk::Entry::builder().placeholder_text("Feed- oder Website-URL").activates_default(true).build();
        let dialog = adw::AlertDialog::builder()
            .heading("Feed hinzufügen")
            .body("URL eingeben; Lesefluss sucht den Feed und zeigt eine Vorschau.")
            .extra_child(&entry)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("add", "Suchen");
        dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("add"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp != "add" {
                return;
            }
            let url = entry.text().trim().to_string();
            if url.is_empty() {
                return;
            }
            app.show_toast("Suche Feed…");
            app.net.discover(url);
        });
    }

    fn show_discovery_dialog(&self, candidates: Vec<provider_local::DiscoverCandidate>) {
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).build();
        for c in &candidates {
            let row = gtk::ListBoxRow::builder()
                .child(&gtk::Label::builder().label(&format!("{} — {}", c.title, c.url)).xalign(0.0).margin_start(8).margin_end(8).margin_top(6).margin_bottom(6).build())
                .build();
            list.append(&row);
        }
        if let Some(r) = list.row_at_index(0) {
            list.select_row(Some(&r));
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Feed gefunden")
            .body("Bitte Feed auswählen und abonnieren.")
            .extra_child(&list)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("sub", "Abonnieren");
        dialog.set_response_appearance("sub", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("sub"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp != "sub" {
                return;
            }
            let Some(idx) = list.selected_row().map(|r| r.index() as usize) else { return };
            let Some(c) = candidates.get(idx) else { return };
            app.subscribe(&c.url, &c.title);
        });
    }

    fn subscribe(&self, url: &str, title: &str) {
        let url = url.to_string();
        let title = title.to_string();
        let w = self.weak();
        self.db_query(
            {
                let url_db = url.clone();
                move |db: &storage::Database| {
                    if let Some(id) = db.feed_id_by_url(&url_db)? {
                        return Ok::<_, storage::StorageError>((id, false));
                    }
                    let accent = ACCENTS[url_db.len() % ACCENTS.len()];
                    let id = db.add_feed("local", &url_db, &title, None, accent)?;
                    Ok((id, true))
                }
            },
            move |app, res: storage::Result<(i64, bool)>| {
                let Ok((feed_id, is_new)) = res else { return };
                if is_new {
                    let url2 = url.clone();
                    app.net.fetch_feed(app.worker.clone(), feed_id, url2, true);
                }
                app.reload_meta_then_select(feed_id);
                let _ = w;
            },
        );
    }

    fn reload_meta_then_select(&self, feed_id: i64) {
        let w = self.weak();
        self.db_query(
            |db| Ok::<_, storage::StorageError>((db.list_feeds()?, db.list_groups()?, db.counts()?)),
            move |app, res: storage::Result<(Vec<FeedRow>, Vec<GroupRow>, Counts)>| {
                let Ok((feeds, groups, counts)) = res else { return };
                {
                    let mut st = app.state.borrow_mut();
                    st.feeds = feeds;
                    st.groups = groups;
                    st.counts = counts;
                }
                app.set_scope(Scope::Feed(feed_id), true);
                let _ = w;
            },
        );
    }

    // ── Thema & Layout ──

    fn apply_theme_now(&self) {
        let dark = adw::StyleManager::default().is_dark();
        let tokens = tokens_for(dark);
        *self.tokens.borrow_mut() = tokens;
        self.css.load_from_string(&gtk_css_for(&tokens));
        let rgba = gtk::gdk::RGBA::new(
            tokens.surface_reader.r as f32 / 255.0,
            tokens.surface_reader.g as f32 / 255.0,
            tokens.surface_reader.b as f32 / 255.0,
            1.0,
        );
        self.reader.webview.set_background_color(&rgba);
    }

    fn apply_theme(&self) {
        self.apply_theme_now();
        self.reload_current(true);
    }

    fn install_width_watcher(&self) {
        let outer = self.outer.clone();
        let inner = self.inner.clone();
        let win = self.window.clone();
        let apply = move || {
            let w = gtk::prelude::NativeExt::surface(&win).map(|s| s.width()).unwrap_or_else(|| win.width());
            outer.set_collapsed(w <= 1119);
            inner.set_collapsed(w <= 779);
        };
        let apply_win = apply.clone();
        self.window.connect_notify_local(None, move |_, pspec| {
            let n = pspec.name();
            if n == "fullscreened" || n == "maximized" {
                apply_win();
            }
        });
        let apply_realize = apply.clone();
        self.window.connect_realize(move |win| {
            if let Some(surface) = gtk::prelude::NativeExt::surface(win) {
                let apply2 = apply_realize.clone();
                surface.connect_notify_local(Some("width"), move |_, _| apply2());
            }
            apply_realize();
        });
        apply();
    }

    // ── Signale ──

    fn wire(&self, factory: gtk::SignalListItemFactory) {
        factory.connect_setup(|_, list_item| {
            let li = list_item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            li.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
        });
        factory.connect_bind(move |_, list_item| {
            let li = list_item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(obj) = li.item() else { return };
            let Ok(boxed) = obj.downcast::<glib::BoxedAnyObject>() else { return };
            let row = boxed.borrow::<ListRow>();
            li.set_child(Some(&list::row_widget(&row)));
        });
        factory.connect_unbind(|_, list_item| {
            let li = list_item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            if let (Some(obj), Some(child)) = (li.item(), li.child()) {
                if let Ok(boxed) = obj.downcast::<glib::BoxedAnyObject>() {
                    let row = boxed.borrow::<ListRow>();
                    list::unregister(&row, &child);
                }
            }
            li.set_child(None::<&gtk::Widget>);
        });

        let w = self.weak();
        self.sidebar_list.connect_row_selected(move |_, row| {
            let Some(app) = w.upgrade() else { return };
            if app.suppress.get() {
                return;
            }
            let Some(row) = row else { return };
            let idx = row.index() as usize;
            let f = {
                let filters = app.sidebar_filters.borrow();
                match filters.get(idx) {
                    Some(Some(f)) => f.clone(),
                    _ => return,
                }
            };
            let reset = matches!(f, Scope::Global);
            app.set_scope(f, reset);
        });

        let w = self.weak();
        self.list_selection.connect_selected_item_notify(move |sel| {
            let Some(app) = w.upgrade() else { return };
            if app.suppress.get() {
                return;
            }
            let cause = app.selection_cause.get();
            app.selection_cause.set(SelectionCause::Unknown);
            if cause != SelectionCause::Keyboard {
                return;
            }
            let Some(obj) = sel.selected_item() else { return };
            let Ok(boxed) = obj.downcast::<glib::BoxedAnyObject>() else { return };
            let row = boxed.borrow::<ListRow>();
            if let Some(a) = row.article() {
                app.schedule_preview(a);
            }
        });

        let w = self.weak();
        self.list_view.connect_activate(move |_, pos| {
            let Some(app) = w.upgrade() else { return };
            let row = app.state.borrow().rows.get(pos as usize).and_then(|r| r.article());
            if let Some(row) = row {
                app.open_article(row, true, true);
            }
        });

        let w = self.weak();
        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_, _, _, _| {
            if let Some(app) = w.upgrade() {
                app.selection_cause.set(SelectionCause::Pointer);
            }
        });
        self.list_view.add_controller(click);
        let w = self.weak();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if let Some(app) = w.upgrade() {
                match key {
                    gtk::gdk::Key::Up | gtk::gdk::Key::Down | gtk::gdk::Key::Home | gtk::gdk::Key::End
                    | gtk::gdk::Key::Page_Up | gtk::gdk::Key::Page_Down => {
                        app.selection_cause.set(SelectionCause::Keyboard);
                    }
                    _ => {}
                }
            }
            glib::Propagation::Proceed
        });
        self.list_view.add_controller(keys);

        for (btn, f) in [
            (self.filter_saved.clone(), Filter::Saved),
            (self.filter_unread.clone(), Filter::Unread),
            (self.filter_all.clone(), Filter::All),
        ] {
            let w = self.weak();
            btn.connect_toggled(move |b| {
                let Some(app) = w.upgrade() else { return };
                if app.syncing_filters.get() {
                    return;
                }
                if !b.is_active() {
                    app.syncing_filters.set(true);
                    b.set_active(true);
                    app.syncing_filters.set(false);
                    return;
                }
                app.set_filter(f);
            });
        }

        let w = self.weak();
        self.list_scroll.vadjustment().connect_value_changed(move |adj| {
            let Some(app) = w.upgrade() else { return };
            let near_bottom = adj.value() + adj.page_size() >= adj.upper() - 400.0;
            if !near_bottom {
                return;
            }
            let st = app.state.borrow();
            let can_more = st.cursor.is_some() && !st.loading_more;
            drop(st);
            if can_more {
                app.state.borrow_mut().loading_more = true;
                app.load_page(true);
            }
        });

        let webview = self.reader.webview.clone();
        self.reader.search_entry.connect_search_changed(move |entry| {
            find_in_view(&webview, &entry.text());
        });
        let webview2 = self.reader.webview.clone();
        self.reader.search_entry.connect_activate(move |_| {
            find_next(&webview2);
        });
        self.reader.search_bar.set_key_capture_widget(Some(&self.window));

        let w = self.weak();
        self.search_entry.connect_search_changed(move |entry| {
            let Some(app) = w.upgrade() else { return };
            let q = entry.text().trim().to_string();
            if let Some(old) = app.search_timer.borrow_mut().take() {
                old.remove();
            }
            let w2 = w.clone();
            let timer = glib::timeout_add_local(Duration::from_millis(150), move || {
                let Some(app) = w2.upgrade() else { return glib::ControlFlow::Break };
                *app.search_timer.borrow_mut() = None;
                {
                    let mut st = app.state.borrow_mut();
                    st.search = if q.is_empty() { None } else { Some(q.clone()) };
                }
                app.list_title.set_title(&app.scope_label_now());
                app.load_page(false);
                glib::ControlFlow::Break
            });
            *app.search_timer.borrow_mut() = Some(timer);
        });

        let w = self.weak();
        self.reader.webview.connect_load_changed(move |_, event| {
            if event == webkit6::LoadEvent::Finished {
                if let Some(app) = w.upgrade() {
                    app.after_load_finished();
                }
            }
        });

        let w = self.weak();
        adw::StyleManager::default().connect_dark_notify(move |_| {
            if let Some(app) = w.upgrade() {
                app.apply_theme();
            }
        });
    }

    fn schedule_preview(&self, row: ArticleRow) {
        if let Some(old) = self.preview_timer.borrow_mut().take() {
            old.remove();
        }
        let w = self.weak();
        let timer = glib::timeout_add_local(Duration::from_millis(220), move || {
            if let Some(app) = w.upgrade() {
                *app.preview_timer.borrow_mut() = None;
                let is_current = app.reader.current.borrow().as_deref() == Some(row.id.as_str());
                if !is_current {
                    app.open_article(row.clone(), false, true);
                }
            }
            glib::ControlFlow::Break
        });
        *self.preview_timer.borrow_mut() = Some(timer);
    }

    // ── Actions ──

    fn register_actions(&self, application: &adw::Application) {
        macro_rules! win_action {
            ($name:expr, |$a:ident| $body:expr) => {{
                let w = self.weak();
                let act = gio::SimpleAction::new($name, None);
                act.connect_activate(move |_, _| {
                    if let Some($a) = w.upgrade() {
                        $body;
                    }
                });
                self.window.add_action(&act);
            }};
        }

        win_action!("toggle-sources", |a| a.outer.set_show_content(false));
        win_action!("reader-back", |a| a.back());
        win_action!("back", |a| a.back());
        win_action!("refresh", |a| a.do_refresh());
        win_action!("add-feed", |a| a.add_feed_dialog());
        win_action!("connect-feedly", |a| a.connect_feedly_dialog());
        win_action!("import-opml", |a| a.import_opml_dialog());
        win_action!("export-opml", |a| a.export_opml());
        win_action!("backup", |a| a.backup_dialog());
        win_action!("restore", |a| a.restore_dialog());
        win_action!("mark-scope-read", |a| a.mark_scope_dialog());
        win_action!("toggle-read", |a| a.toggle_read());
        win_action!("toggle-saved", |a| a.toggle_saved());
        win_action!("open-external", |a| a.open_external());
        win_action!("copy-link", |a| a.copy_link());
        win_action!("zoom-in", |a| a.zoom(2.0));
        win_action!("zoom-out", |a| a.zoom(-2.0));
        win_action!("zoom-reset", |a| a.zoom_reset());
        win_action!("find", |a| a.open_find());
        win_action!("article-search", |a| {
            a.search_bar.set_search_mode(true);
            a.search_entry.grab_focus();
        });
        win_action!("reader-retry", |a| a.reload_current(false));
        win_action!("undo", |a| a.undo());
        win_action!("close", |a| a.window.close());
        win_action!("next-article", |a| a.move_selection(1));
        win_action!("prev-article", |a| a.move_selection(-1));
        win_action!("next-unread", |a| a.move_unread(1));
        win_action!("prev-unread", |a| a.move_unread(-1));
        win_action!("focus-next-pane", |a| a.focus_pane(1));
        win_action!("focus-prev-pane", |a| a.focus_pane(-1));

        let theme_action = gio::SimpleAction::new_stateful(
            "theme",
            Some(&String::static_variant_type()),
            &"system".to_variant(),
        );
        theme_action.connect_activate(|action, param| {
            let mode = param.and_then(|p| p.get::<String>()).unwrap_or_default();
            let scheme = match mode.as_str() {
                "dark" => adw::ColorScheme::ForceDark,
                "light" => adw::ColorScheme::ForceLight,
                _ => adw::ColorScheme::PreferLight,
            };
            adw::StyleManager::default().set_color_scheme(scheme);
            if let Some(p) = param {
                action.set_state(p);
            }
        });
        application.add_action(&theme_action);

        let quit = gio::SimpleAction::new("quit", None);
        let app_for_quit = application.clone();
        quit.connect_activate(move |_, _| app_for_quit.quit());
        application.add_action(&quit);

        application.set_accels_for_action("win.next-article", &["j"]);
        application.set_accels_for_action("win.prev-article", &["k"]);
        application.set_accels_for_action("win.next-unread", &["n"]);
        application.set_accels_for_action("win.prev-unread", &["p"]);
        application.set_accels_for_action("win.toggle-read", &["m"]);
        application.set_accels_for_action("win.toggle-saved", &["s"]);
        application.set_accels_for_action("win.open-external", &["o"]);
        application.set_accels_for_action("win.find", &["<Control>f"]);
        application.set_accels_for_action("win.article-search", &["<Control>l"]);
        application.set_accels_for_action("win.refresh", &["<Control>r"]);
        application.set_accels_for_action("win.add-feed", &["<Control>n"]);
        application.set_accels_for_action("win.mark-scope-read", &["<Control><Shift>m"]);
        application.set_accels_for_action("win.undo", &["<Control>z"]);
        application.set_accels_for_action("win.zoom-in", &["<Control>plus", "<Control>equal", "<Control>KP_Add"]);
        application.set_accels_for_action("win.zoom-out", &["<Control>minus", "<Control>KP_Subtract"]);
        application.set_accels_for_action("win.zoom-reset", &["<Control>0"]);
        application.set_accels_for_action("win.focus-next-pane", &["F6"]);
        application.set_accels_for_action("win.focus-prev-pane", &["<Shift>F6"]);
        application.set_accels_for_action("win.back", &["<Alt>Left"]);
        application.set_accels_for_action("win.close", &["<Control>w"]);
        application.set_accels_for_action("app.quit", &["<Control>q"]);
    }
}
