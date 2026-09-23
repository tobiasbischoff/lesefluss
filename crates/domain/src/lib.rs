pub type AccountId = String;
pub type FeedId = String;
pub type GroupId = String;
pub type ArticleId = String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind {
    Local,
    Feedly,
}

#[derive(Clone, Debug)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub parent: Option<GroupId>,
}

#[derive(Clone, Debug)]
pub struct Feed {
    pub id: FeedId,
    pub title: String,
    pub website: Option<String>,
    pub accent: String,
    pub groups: Vec<GroupId>,
}

#[derive(Clone, Debug)]
pub struct ArticleMeta {
    pub id: ArticleId,
    pub feed_id: FeedId,
    pub title: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub published_at: i64,
    pub excerpt: String,
    pub unread: bool,
    pub saved: bool,
    pub has_image: bool,
}

#[derive(Clone, Debug)]
pub struct ArticleContent {
    pub html: String,
}
