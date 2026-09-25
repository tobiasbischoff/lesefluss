//! Koordination der Feedly-Zyklen pro Konto.
//!
//! Erst-Sync, Delta-Sync, Outbox-Versand und serverweite Aktionen laufen über
//! denselben Coordinator: höchstens ein Auftrag gleichzeitig, weitere Wünsche
//! werden mit ihrer Priorität vorgemerkt. Auth- und Quotenpausen gelten für
//! alle Wege, auch für den manuellen Refresh.

use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Job {
    /// Periodischer Hintergrundlauf.
    Scheduled,
    /// Delta-Sync nach Benutzerwunsch.
    Refresh,
    /// Versand der wartenden Outbox-Zeilen.
    Outbox,
    /// Erst-Sync nach dem Verbinden.
    Initial,
    /// Große serverseitige Aktion, z. B. „alles gelesen“.
    ServerAction,
}

impl Job {
    /// Höhere Zahl gewinnt, wenn mehrere Wünsche anstehen.
    pub fn priority(self) -> u8 {
        match self {
            Job::Scheduled => 0,
            Job::Outbox => 1,
            Job::Refresh => 2,
            Job::ServerAction => 3,
            Job::Initial => 4,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pause {
    /// Authentifizierung nötig; alle Läufe warten auf neue Anmeldung.
    Auth,
    /// Drosselung bis zu diesem Zeitpunkt (ms).
    Quota(i64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// Jetzt starten; `run_id` kennzeichnet den Lauf bis zum Abschluss.
    Start { run_id: u64 },
    /// Läuft bereits; der Wunsch ist vorgemerkt.
    Queued,
    /// Pause aktiv; der Wunsch wird verworfen und erst nach erneutem Anfordern
    /// erneut betrachtet.
    Paused(Pause),
    /// Gesperrt (z. B. abgemeldet).
    Blocked,
}

#[derive(Debug, Default)]
struct AccountState {
    running: Option<Job>,
    /// Laufkennung des aktuellen Laufs; steigt mit jedem Start.
    run_id: u64,
    queued: Option<Job>,
    pause: Option<Pause>,
    blocked: bool,
    /// Lauf, dessen Arbeit nach `block()` verworfen werden muss.
    cancel_current: bool,
}

#[derive(Debug, Default)]
pub struct SyncCoordinator {
    accounts: HashMap<String, AccountState>,
    now_ms: i64,
}

/// Ergebnis eines abgeschlossenen Laufs: der Coordinator reserviert den Folgelauf
/// und liefert ihn **mit** seiner Laufkennung zurück. Der Aufrufer startet diesen
/// Auftrag direkt, ohne ihn erneut anzufordern.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReservedRun {
    pub job: Job,
    pub run_id: u64,
}

impl SyncCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_now(&mut self, now_ms: i64) {
        self.now_ms = now_ms;
    }

    fn state(&mut self, account: &str) -> &mut AccountState {
        self.accounts.entry(account.to_string()).or_default()
    }

    fn pause_active(state: &AccountState, now: i64) -> Option<Pause> {
        match state.pause {
            Some(Pause::Auth) => Some(Pause::Auth),
            Some(Pause::Quota(until)) if until > now => Some(Pause::Quota(until)),
            _ => None,
        }
    }

    /// Fordert einen Lauf an. Liegt bereits einer vor, wird der wichtigere
    /// Wunsch vorgemerkt; ein Refresh erhöht nur die Priorität.
    pub fn request(&mut self, account: &str, job: Job) -> Decision {
        let now = self.now_ms;
        let state = self.state(account);
        if state.blocked {
            return Decision::Blocked;
        }
        if let Some(pause) = Self::pause_active(state, now) {
            return Decision::Paused(pause);
        }
        if state.running.is_some() {
            state.queued = match state.queued {
                Some(pending) if pending.priority() >= job.priority() => Some(pending),
                _ => Some(job),
            };
            return Decision::Queued;
        }
        state.run_id += 1;
        state.running = Some(job);
        state.cancel_current = false;
        Decision::Start {
            run_id: state.run_id,
        }
    }

    /// Meldet das Ende eines Laufs und reserviert den Folgelauf, falls einer
    /// wartete. Der Rückgabewert ist der **einzige** Startauftrag dafür; er darf
    /// nicht erneut über `request` angemeldet werden.
    pub fn finish(&mut self, account: &str, now_ms: i64) -> Option<ReservedRun> {
        self.now_ms = now_ms;
        let state = self.state(account);
        state.running = None;
        state.cancel_current = false;
        if Self::pause_active(state, now_ms).is_some() {
            state.queued = None;
            return None;
        }
        match state.queued.take() {
            Some(next) => {
                state.run_id += 1;
                state.running = Some(next);
                Some(ReservedRun {
                    job: next,
                    run_id: state.run_id,
                })
            }
            None => None,
        }
    }

    /// Meldet einen Abbruch (z. B. Logout) und verwirft vorgemerkte Arbeit.
    /// Der laufende Auftrag wird zum Invalidieren markiert.
    pub fn abort(&mut self, account: &str) -> Option<Job> {
        let state = self.state(account);
        let running = state.running;
        state.running = None;
        state.queued = None;
        state.cancel_current = running.is_some();
        running
    }

    /// Der aktuell laufende oder reservierte Auftrag samt Laufkennung, ohne einen
    /// neuen Lauf zu starten. Wird vom Dispatcher nach `finish` benutzt.
    pub fn reserved_run(&self, account: &str) -> Option<(Job, u64)> {
        self.accounts
            .get(account)
            .and_then(|s| s.running.map(|job| (job, s.run_id)))
    }

    /// Laufkennung des aktuellen Laufs; `None`, wenn nichts läuft.
    pub fn current_run(&self, account: &str) -> Option<u64> {
        self.accounts
            .get(account)
            .filter(|s| s.running.is_some())
            .map(|s| s.run_id)
    }

    /// Wurde der laufende Auftrag nach einem Abbruch invalidiert?
    pub fn is_cancelled(&self, account: &str) -> bool {
        self.accounts
            .get(account)
            .map(|s| s.cancel_current)
            .unwrap_or(false)
    }

    pub fn pause(&mut self, account: &str, pause: Pause) {
        let state = self.state(account);
        state.pause = Some(pause);
        state.queued = None;
    }

    /// Sorgt dafür, dass ein Auth-/Quota-Stopp auch den laufenden Versand beendet.
    pub fn stop_running(&mut self, account: &str) -> Option<Job> {
        let state = self.state(account);
        let running = state.running;
        state.running = None;
        state.queued = None;
        state.cancel_current = running.is_some();
        running
    }

    /// Hebt eine Pause auf (neue Anmeldung, Zeit abgelaufen).
    pub fn resume(&mut self, account: &str) {
        let state = self.state(account);
        state.pause = None;
    }

    /// Abmelden: alles wird verworfen, bis `resume` nach neuem Verbinden.
    pub fn block(&mut self, account: &str) {
        let state = self.state(account);
        state.blocked = true;
        state.running = None;
        state.queued = None;
        state.pause = None;
    }

    pub fn unblock(&mut self, account: &str) {
        self.state(account).blocked = false;
    }

    pub fn running(&self, account: &str) -> Option<Job> {
        self.accounts.get(account).and_then(|s| s.running)
    }

    pub fn queued(&self, account: &str) -> Option<Job> {
        self.accounts.get(account).and_then(|s| s.queued)
    }

    pub fn pause_state(&self, account: &str) -> Option<Pause> {
        self.accounts.get(account).and_then(|s| s.pause)
    }

    pub fn is_blocked(&self, account: &str) -> bool {
        self.accounts
            .get(account)
            .map(|s| s.blocked)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(c: &mut SyncCoordinator, account: &str, job: Job) -> Option<u64> {
        match c.request(account, job) {
            Decision::Start { run_id } => Some(run_id),
            other => panicunerwartet(other),
        }
    }

    fn panicunerwartet(d: Decision) -> ! {
        panic!("unerwartete Entscheidung: {d:?}")
    }

    #[test]
    fn laufende_zyklen_werden_nicht_verdoppelt() {
        let mut c = SyncCoordinator::new();
        let first = start(&mut c, "feedly-1", Job::Initial).expect("Start");
        assert_eq!(c.request("feedly-1", Job::Scheduled), Decision::Queued);
        assert_eq!(c.running("feedly-1"), Some(Job::Initial));
        assert_eq!(c.current_run("feedly-1"), Some(first));
    }

    #[test]
    fn der_wichtigere_wunsch_gewinnt() {
        let mut c = SyncCoordinator::new();
        start(&mut c, "feedly-1", Job::Initial);
        c.request("feedly-1", Job::Scheduled);
        c.request("feedly-1", Job::ServerAction);
        assert_eq!(c.queued("feedly-1"), Some(Job::ServerAction));
        let reserved = c.finish("feedly-1", 0).expect("Folgelauf reserviert");
        assert_eq!(reserved.job, Job::ServerAction);
        assert_eq!(c.running("feedly-1"), Some(Job::ServerAction));
    }

    #[test]
    fn niedrigere_prioritaet_ersetzt_den_wunsch_nicht() {
        let mut c = SyncCoordinator::new();
        start(&mut c, "feedly-1", Job::Initial);
        c.request("feedly-1", Job::Refresh);
        c.request("feedly-1", Job::Scheduled);
        assert_eq!(c.queued("feedly-1"), Some(Job::Refresh));
    }

    /// A2: Start A, Refresh während A, Abschluss A → genau **ein** reservierter
    /// Start B, Abschluss B, danach wieder ein freier Start. Der reservierte
    /// Auftrag darf nicht erneut angefordert werden.
    #[test]
    fn reservierter_folgelauf_wird_genau_einmal_gestartet() {
        let mut c = SyncCoordinator::new();
        let a = start(&mut c, "k", Job::Initial).expect("A");
        assert_eq!(c.request("k", Job::Refresh), Decision::Queued);
        let b = c.finish("k", 10).expect("B reserviert");
        assert_ne!(b.run_id, a, "jeder Lauf hat eine eigene Kennung");
        // Der Aufrufer startet B direkt. Ein erneutes Anfordern würde B nur
        // wieder vormerken und keinen Netzwerkstart auslösen.
        assert_eq!(c.current_run("k"), Some(b.run_id));
        assert!(c.finish("k", 20).is_none(), "kein dritter Lauf ohne Wunsch");
        let c_run = start(&mut c, "k", Job::Refresh).expect("nachfolgender Lauf");
        assert!(c_run > b.run_id);
    }

    #[test]
    fn auch_im_fehlerfall_bleibt_kein_haengender_lauf() {
        let mut c = SyncCoordinator::new();
        start(&mut c, "k", Job::Initial);
        c.request("k", Job::Outbox);
        // Fehlerpfad: der Abschluss wird auch dort aufgerufen.
        let reserved = c.finish("k", 0).expect("Folgelauf nach Fehler");
        assert_eq!(reserved.job, Job::Outbox);
        assert!(c.finish("k", 0).is_none());
    }

    #[test]
    fn auth_pause_hält_auch_den_manuellen_refresh_an() {
        let mut c = SyncCoordinator::new();
        c.pause("feedly-1", Pause::Auth);
        assert_eq!(
            c.request("feedly-1", Job::Refresh),
            Decision::Paused(Pause::Auth)
        );
        assert_eq!(
            c.request("feedly-1", Job::Initial),
            Decision::Paused(Pause::Auth)
        );
        c.resume("feedly-1");
        assert!(matches!(
            c.request("feedly-1", Job::Refresh),
            Decision::Start { .. }
        ));
    }

    #[test]
    fn quotenpause_gilt_nur_bis_zum_zeitpunkt() {
        let mut c = SyncCoordinator::new();
        c.set_now(1_000);
        c.pause("feedly-1", Pause::Quota(2_000));
        assert_eq!(
            c.request("feedly-1", Job::Refresh),
            Decision::Paused(Pause::Quota(2_000))
        );
        c.set_now(3_000);
        assert!(matches!(
            c.request("feedly-1", Job::Refresh),
            Decision::Start { .. }
        ));
    }

    #[test]
    fn pause_verwirft_bereits_vorgemerkte_arbeit() {
        let mut c = SyncCoordinator::new();
        start(&mut c, "feedly-1", Job::Initial);
        c.request("feedly-1", Job::Refresh);
        c.pause("feedly-1", Pause::Auth);
        assert!(c.finish("feedly-1", 0).is_none());
        assert_eq!(c.queued("feedly-1"), None);
    }

    #[test]
    fn abmelden_sperrt_bis_zur_erneuten_verbindung() {
        let mut c = SyncCoordinator::new();
        start(&mut c, "feedly-1", Job::Refresh);
        c.block("feedly-1");
        assert!(c.is_blocked("feedly-1"));
        assert_eq!(c.request("feedly-1", Job::Refresh), Decision::Blocked);
        c.unblock("feedly-1");
        assert!(matches!(
            c.request("feedly-1", Job::Refresh),
            Decision::Start { .. }
        ));
    }

    #[test]
    fn abmelden_invalidiert_den_laufenden_auftrag() {
        let mut c = SyncCoordinator::new();
        start(&mut c, "feedly-1", Job::Outbox);
        assert!(
            c.abort("feedly-1").is_some(),
            "der laufende Auftrag wird gemeldet"
        );
        assert!(
            c.is_cancelled("feedly-1"),
            "weitere Arbeit muss verworfen werden"
        );
        assert!(c.current_run("feedly-1").is_none());
        assert!(c.finish("feedly-1", 0).is_none());
    }

    #[test]
    fn auth_oder_quota_beendet_den_laufenden_versand() {
        let mut c = SyncCoordinator::new();
        start(&mut c, "feedly-1", Job::Outbox);
        assert!(c.stop_running("feedly-1").is_some());
        assert!(c.is_cancelled("feedly-1"));
        c.pause("feedly-1", Pause::Auth);
        assert_eq!(
            c.request("feedly-1", Job::Refresh),
            Decision::Paused(Pause::Auth)
        );
    }

    #[test]
    fn konten_synchronisieren_unabhaengig() {
        let mut c = SyncCoordinator::new();
        let a = start(&mut c, "konto-a", Job::Refresh).expect("A");
        let b = start(&mut c, "konto-b", Job::Refresh).expect("B");
        assert!(a > 0 && b > 0, "beide Läufe haben eine Kennung");
        assert_eq!(c.running("konto-a"), Some(Job::Refresh));
        assert_eq!(c.running("konto-b"), Some(Job::Refresh));
    }
}
