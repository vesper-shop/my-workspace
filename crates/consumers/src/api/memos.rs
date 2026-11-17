use super::{ApiError, Client, send};
use futures_util::future::try_join_all;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;

const ENDPOINT: &str = "https://noted-this.date/api";
pub const PAGE_SIZE: usize = 50;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Private,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favorite: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Memo {
    pub id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub visibility: Visibility,
    pub pinned: bool,
    pub favorite: bool,
    pub archived: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoView {
    #[serde(flatten)]
    pub memo: Memo,
    pub html: String,
    pub metadata_complete: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub memos: Vec<MemoView>,
    pub next_cursor: Option<String>,
}

#[derive(Default)]
pub struct ListFilters {
    pub limit: Option<usize>,
    pub search: Option<String>,
    pub tags: Vec<String>,
    pub sort_by_updated: bool,
    pub archived_only: bool,
    pub favorites_only: bool,
}

#[derive(Deserialize)]
struct Meta {
    id: String,
    occurred: String,
    visibility: Visibility,
    pinned: Option<String>,
    favorite: Option<String>,
    archived: Option<String>,
    tags: Vec<String>,
    photos: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Document {
    meta: Meta,
    body: String,
}

#[derive(Deserialize)]
struct DocumentPage {
    documents: Vec<Document>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct SearchEntry {
    id: String,
    photos: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct SearchPage {
    entries: Vec<SearchEntry>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct Created {
    id: String,
}

#[derive(Deserialize)]
struct TagResponse {
    tags: Vec<TagCount>,
}

#[derive(Deserialize)]
struct XPostResponse {
    tweet: XPost,
}

#[derive(Deserialize)]
struct XPost {
    text: String,
    url: String,
    author: XAuthor,
    #[serde(default)]
    media: XMedia,
}

#[derive(Deserialize)]
struct XAuthor {
    name: String,
    screen_name: String,
}

#[derive(Default, Deserialize)]
struct XMedia {
    #[serde(default)]
    photos: Vec<XPhoto>,
}

#[derive(Deserialize)]
struct XPhoto {
    #[serde(rename = "type")]
    kind: String,
    url: String,
    #[serde(rename = "altText", default)]
    alt_text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TagCount {
    pub name: String,
    pub count: u64,
}

impl Document {
    fn into_view(self) -> MemoView {
        let Document { meta, body } = self;
        let content = with_tags(&body, &meta.tags);
        MemoView {
            metadata_complete: true,
            html: markdown::render_memo(&strip_tags(&content)),
            memo: Memo {
                id: meta.id,
                content,
                tags: meta.tags,
                created_at: meta.occurred.clone(),
                updated_at: meta.occurred,
                visibility: meta.visibility,
                pinned: meta.pinned.is_some(),
                favorite: meta.favorite.is_some(),
                archived: meta.archived.is_some(),
            },
        }
    }
}

fn with_tags(body: &str, tags: &[String]) -> String {
    if tags.is_empty() {
        return body.trim_end().to_owned();
    }
    let line = tags
        .iter()
        .map(|tag| format!("#{tag}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{}\n\n{line}", body.trim_end())
}

fn body_of(content: &str) -> String {
    let mut lines: Vec<&str> = content.trim_end().lines().collect();
    while lines.last().is_some_and(|line| {
        let words: Vec<&str> = line.split_whitespace().collect();
        !words.is_empty() && words.iter().all(|word| is_hashtag(word))
    }) {
        lines.pop();
    }
    lines.join("\n").trim_end().to_owned()
}

fn is_hashtag(word: &str) -> bool {
    word.strip_prefix('#').is_some_and(|name| {
        !name.is_empty() && !name.starts_with('#') && name.chars().all(is_tag_character)
    })
}

pub fn extract_tags(content: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for word in content.split_whitespace() {
        if is_hashtag(word) {
            let tag = word[1..].to_lowercase();
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
    }
    tags
}

fn entry_url(id: &str) -> Result<reqwest::Url, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::Protocol("a memo ID is required".to_owned()));
    }
    let mut url = reqwest::Url::parse(&format!("{ENDPOINT}/entries/"))
        .map_err(|_| ApiError::Protocol("the Vesper endpoint is invalid".to_owned()))?;
    url.path_segments_mut()
        .map_err(|_| ApiError::Protocol("the Vesper endpoint cannot contain an ID".to_owned()))?
        .pop_if_empty()
        .push(id);
    Ok(url)
}

fn shelf(filters: &ListFilters) -> &'static str {
    if filters.archived_only {
        "archive"
    } else if filters.favorites_only {
        "favorites"
    } else {
        "all"
    }
}

pub async fn list(cursor: Option<String>, filters: &ListFilters) -> Result<Page, ApiError> {
    let client = Client::load().await?;
    let mut query = vec![("in", shelf(filters).to_owned())];
    if let [tag] = filters.tags.as_slice() {
        query.push(("tag", tag.clone()));
    }
    if let Some(before) = cursor {
        query.push(("before", before));
    }
    let search = filters
        .search
        .as_deref()
        .filter(|search| !search.is_empty());
    let (documents, next) = match search {
        Some(search) => {
            query.push(("q", search.to_owned()));
            let response = send(
                client
                    .http
                    .get(format!("{ENDPOINT}/entries"))
                    .bearer_auth(&client.token)
                    .query(&query),
                "search memos",
            )
            .await?;
            let page: SearchPage = response.json().await?;
            let ids: Vec<String> = page
                .entries
                .into_iter()
                .filter(|entry| entry.photos.is_empty())
                .map(|entry| entry.id)
                .collect();
            let documents = try_join_all(ids.iter().map(|id| fetch(&client, id))).await?;
            (documents, page.next)
        }
        None => {
            let response = send(
                client
                    .http
                    .get(format!("{ENDPOINT}/entries/documents"))
                    .bearer_auth(&client.token)
                    .query(&query),
                "list memos",
            )
            .await?;
            let page: DocumentPage = response.json().await?;
            (page.documents, page.next)
        }
    };
    let memos = documents
        .into_iter()
        .filter(|document| document.meta.photos.is_empty())
        .filter(|document| {
            filters.tags.len() < 2
                || filters.tags.iter().any(|tag| {
                    document
                        .meta
                        .tags
                        .iter()
                        .any(|own| own.eq_ignore_ascii_case(tag))
                })
        })
        .map(Document::into_view)
        .collect();
    Ok(Page {
        memos,
        next_cursor: next,
    })
}

async fn fetch(client: &Client, id: &str) -> Result<Document, ApiError> {
    let response = send(
        client.http.get(entry_url(id)?).bearer_auth(&client.token),
        "read memo",
    )
    .await?;
    Ok(response.json().await?)
}

pub async fn read(id: &str) -> Result<MemoView, ApiError> {
    let client = Client::load().await?;
    Ok(fetch(&client, id).await?.into_view())
}

pub async fn tags() -> Result<Vec<TagCount>, ApiError> {
    let client = Client::load().await?;
    let response = send(
        client
            .http
            .get(format!("{ENDPOINT}/tags"))
            .bearer_auth(&client.token),
        "list memo tags",
    )
    .await?;
    let result: TagResponse = response.json().await?;
    Ok(result.tags)
}

pub async fn create(content: &str, visibility: Visibility) -> Result<MemoView, ApiError> {
    submit(content, visibility, false).await
}

async fn submit(
    content: &str,
    visibility: Visibility,
    favorite: bool,
) -> Result<MemoView, ApiError> {
    let client = Client::load().await?;
    let response = send(
        client
            .http
            .post(format!("{ENDPOINT}/entries"))
            .bearer_auth(&client.token)
            .json(&json!({
                "body": body_of(content),
                "tags": extract_tags(content),
                "visibility": visibility,
                "favorite": favorite
            })),
        "create memo",
    )
    .await?;
    if response.status() != StatusCode::CREATED {
        return Err(ApiError::Status {
            operation: "create memo",
            status: response.status(),
        });
    }
    let created: Created = response.json().await?;
    Ok(fetch(&client, &created.id).await?.into_view())
}

pub async fn import_x(source_url: &str, visibility: Visibility) -> Result<MemoView, ApiError> {
    let post_id = parse_x_post_id(source_url)
        .ok_or_else(|| ApiError::Protocol("a valid X post URL is required".to_owned()))?;
    let http = reqwest::Client::builder()
        .timeout(super::REQUEST_TIMEOUT)
        .user_agent(concat!("Vesper/", env!("CARGO_PKG_VERSION"), " X import"))
        .build()?;
    let response = send(
        http.get(format!("https://api.fxtwitter.com/status/{post_id}")),
        "import X post",
    )
    .await?;
    let post: XPostResponse = response.json().await?;
    let content = format_x_post(post.tweet)?;
    submit(&content, visibility, true).await
}

pub async fn update(id: &str, input: &Update) -> Result<MemoView, ApiError> {
    let url = entry_url(id)?;
    let client = Client::load().await?;
    let tags = match (&input.content, &input.tags) {
        (_, Some(tags)) if !tags.is_empty() => Some(tags.clone()),
        (Some(content), _) => Some(extract_tags(content)),
        (None, tags) => tags.clone(),
    };
    send(
        client
            .http
            .patch(url)
            .bearer_auth(&client.token)
            .json(&json!({
                "body": input.content.as_deref().map(body_of),
                "tags": tags,
                "visibility": input.visibility,
                "pinned": input.pinned,
                "favorite": input.favorite,
                "archived": input.archived
            })),
        "update memo",
    )
    .await?;
    Ok(fetch(&client, id).await?.into_view())
}

pub async fn delete(id: &str) -> Result<(), ApiError> {
    let url = entry_url(id)?;
    let client = Client::load().await?;
    send(
        client.http.delete(url).bearer_auth(&client.token),
        "delete memo",
    )
    .await?;
    Ok(())
}

pub fn strip_tags(content: &str) -> String {
    let characters: Vec<char> = content.chars().collect();
    let mut output = String::with_capacity(content.len());
    let mut index = 0;
    while index < characters.len() {
        if characters[index] == '#'
            && (index == 0 || characters[index - 1].is_whitespace())
            && characters
                .get(index + 1)
                .is_some_and(|character| *character != '#' && is_tag_character(*character))
        {
            let mut end = index + 1;
            while end < characters.len() && is_tag_character(characters[end]) {
                end += 1;
            }
            if end == characters.len() || characters[end].is_whitespace() {
                index = end;
                continue;
            }
        }
        output.push(characters[index]);
        index += 1;
    }
    let mut compact = String::with_capacity(output.len());
    let mut horizontal_space = false;
    for character in output.chars() {
        if matches!(character, ' ' | '\t') {
            if horizontal_space {
                continue;
            }
            horizontal_space = true;
        } else {
            horizontal_space = false;
        }
        compact.push(character);
    }
    compact.trim().to_owned()
}

fn is_tag_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-' | '/')
}

fn parse_x_post_id(source: &str) -> Option<String> {
    let url = reqwest::Url::parse(source).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let host = url
        .host_str()?
        .strip_prefix("www.")
        .unwrap_or(url.host_str()?);
    if !matches!(host, "x.com" | "twitter.com") {
        return None;
    }
    let segments: Vec<_> = url.path_segments()?.collect();
    if segments.len() != 3 || segments[1] != "status" {
        return None;
    }
    let id = segments[2];
    if !(2..=20).contains(&id.len()) {
        return None;
    }
    if !id.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    Some(id.to_owned())
}

fn format_x_post(post: XPost) -> Result<String, ApiError> {
    const INVALID_POST: &str = "X returned an unsupported post response";
    let post_url =
        reqwest::Url::parse(&post.url).map_err(|_| ApiError::Protocol(INVALID_POST.to_owned()))?;
    if post.text.trim().is_empty() || post.author.name.trim().is_empty() {
        return Err(ApiError::Protocol(INVALID_POST.to_owned()));
    }
    if post.author.screen_name.trim().is_empty() {
        return Err(ApiError::Protocol(INVALID_POST.to_owned()));
    }
    if !post
        .author
        .screen_name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(ApiError::Protocol(INVALID_POST.to_owned()));
    }
    if parse_x_post_id(&post.url).is_none() {
        return Err(ApiError::Protocol(INVALID_POST.to_owned()));
    }
    let mut content = escape_markdown_text(post.text.trim());
    let photos: Result<Vec<_>, _> = post
        .media
        .photos
        .into_iter()
        .filter(|photo| photo.kind == "photo")
        .map(|photo| {
            let url = reqwest::Url::parse(&photo.url).map_err(|_| {
                ApiError::Protocol("X returned an unsupported photo URL".to_owned())
            })?;
            if url.scheme() != "https" || url.host_str() != Some("pbs.twimg.com") {
                return Err(ApiError::Protocol(
                    "X returned an unsupported photo URL".to_owned(),
                ));
            }
            let alt = escape_markdown_text(
                &photo
                    .alt_text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            Ok(format!("![{alt}](<{}>)", url))
        })
        .collect();
    let photos = photos?;
    if !photos.is_empty() {
        content.push_str("\n\n");
        content.push_str(&photos.join("\n\n"));
    }
    content.push_str(&format!(
        "\n\n— {} (@{})\n{}",
        escape_markdown_text(post.author.name.trim()),
        post.author.screen_name.trim(),
        post_url
    ));
    Ok(content)
}

fn escape_markdown_text(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if matches!(
                character,
                '\\' | '`'
                    | '*'
                    | '_'
                    | '{'
                    | '}'
                    | '['
                    | ']'
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '#'
                    | '+'
                    | '-'
                    | '.'
                    | '!'
                    | '|'
            ) {
                Some('\\')
            } else {
                None
            }
            .into_iter()
            .chain(std::iter::once(character))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_entry_ids() {
        assert_eq!(entry_url("item").unwrap().path(), "/api/entries/item");
        assert_eq!(
            entry_url("../settings").unwrap().path(),
            "/api/entries/..%2Fsettings"
        );
        assert_eq!(entry_url("a b").unwrap().path(), "/api/entries/a%20b");
    }

    #[tokio::test]
    async fn rejects_blank_memo_ids() {
        for id in ["", "   "] {
            assert!(
                matches!(
                    update(id, &Update::default()).await,
                    Err(ApiError::Protocol(_))
                ),
                "{id}"
            );
            assert!(
                matches!(delete(id).await, Err(ApiError::Protocol(_))),
                "{id}"
            );
        }
    }

    #[test]
    fn strips_memo_tags() {
        let content = "Body #Rust #rust #长文\nnext line";
        assert_eq!(strip_tags(content), "Body \nnext line");
    }

    #[test]
    fn projects_entry_into_memo() {
        let document: Document = serde_json::from_value(serde_json::json!({
            "meta": {
                "id": "entry",
                "occurred": "2026-08-25T00:00:00.000Z",
                "visibility": "private",
                "pinned": null,
                "favorite": "2026-08-26T00:00:00.000Z",
                "archived": null,
                "tags": ["rust"],
                "photos": []
            },
            "body": "Complete API body"
        }))
        .unwrap();
        let view = document.into_view();
        assert_eq!(view.memo.content, "Complete API body\n\n#rust");
        assert!(view.memo.favorite && !view.memo.pinned);
        assert!(view.html.contains("Complete API body"));
        assert_eq!(
            serde_json::to_value(&view).unwrap()["metadataComplete"],
            true
        );
    }

    #[test]
    fn separates_body_and_tags() {
        let content = "Line #inline\n\n#daily #rust";
        assert_eq!(body_of(content), "Line #inline");
        assert_eq!(extract_tags(content), ["inline", "daily", "rust"]);
    }

    #[test]
    fn accepts_x_status_urls() {
        assert_eq!(
            parse_x_post_id("https://x.com/Cloudflare/status/2084626665670398004"),
            Some("2084626665670398004".to_owned())
        );
        assert_eq!(
            parse_x_post_id("https://www.twitter.com/user/status/12345?ref=home"),
            Some("12345".to_owned())
        );
        assert_eq!(
            parse_x_post_id("https://example.com/user/status/12345"),
            None
        );
        assert_eq!(parse_x_post_id("https://x.com/Cloudflare"), None);
    }

    #[test]
    fn formats_x_photos() {
        let content = format_x_post(XPost {
            text: "A useful post <script>alert(1)</script> [click](javascript:alert(1))".to_owned(),
            url: "https://x.com/Cloudflare/status/2084626665670398004".to_owned(),
            author: XAuthor {
                name: "Cloudflare *team*".to_owned(),
                screen_name: "Cloudflare".to_owned(),
            },
            media: XMedia {
                photos: vec![XPhoto {
                    kind: "photo".to_owned(),
                    url: "https://pbs.twimg.com/media/example.jpg".to_owned(),
                    alt_text: "An [architecture] diagram".to_owned(),
                }],
            },
        })
        .unwrap();

        assert!(content.contains("![An \\[architecture\\] diagram]"));
        assert!(content.contains("\\<script\\>alert\\(1\\)\\</script\\>"));
        assert!(content.contains("\\[click\\]\\(javascript:alert\\(1\\)\\)"));
        assert!(content.contains("— Cloudflare \\*team\\* (@Cloudflare)"));
        let html = markdown::render_memo(&content);
        assert!(!html.contains("<script>"));
        assert!(!html.contains("href=\"javascript:"));
    }
}
