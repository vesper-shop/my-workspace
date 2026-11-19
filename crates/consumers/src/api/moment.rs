use super::{ApiError, Client, VESPER_API, resource_url, send};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod exif;
mod media;

pub(super) use media::Error as MediaError;

pub const KEY_PREFIX: &str = "img/vesper/";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Geo {
    pub lat: f64,
    pub lng: f64,
}

/// Local photo metadata; inspecting a source performs no upload or credential reads.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoMetadata {
    pub captured_at: Option<String>,
    pub geo: Option<Geo>,
}

pub async fn read_metadata(source: Vec<u8>) -> Result<PhotoMetadata, ApiError> {
    let task = tokio::task::spawn_blocking(move || {
        let heif = media::is_heif_source(&source)?;
        let metadata = exif::read(&source, heif);
        Ok::<_, MediaError>(PhotoMetadata {
            captured_at: metadata.captured_at,
            geo: metadata.geo,
        })
    })
    .await
    .map_err(|_| ApiError::Protocol("photo metadata reader stopped unexpectedly".to_owned()))?;
    task.map_err(ApiError::Media)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Photo {
    pub id: String,
    pub entry_id: String,
    pub image_key: String,
    pub thumbnail_key: String,
    pub thumb_hash: Option<String>,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub tags: Vec<String>,
    pub date: Option<String>,
    pub description: Option<String>,
    pub geo: Option<Geo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub photos: Vec<Photo>,
    pub total: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Upload {
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub date: Option<String>,
    pub geo: Option<Geo>,
}

impl Upload {
    fn validate(&self) -> Result<(), ApiError> {
        const INVALID_METADATA: &str = "photo upload metadata is invalid";
        if self.title.trim().is_empty() || self.title.chars().count() > 120 {
            return Err(ApiError::Protocol(INVALID_METADATA.to_owned()));
        }
        if self
            .description
            .as_ref()
            .is_some_and(|description| description.chars().count() > 500)
        {
            return Err(ApiError::Protocol(INVALID_METADATA.to_owned()));
        }
        if self.tags.len() > 10 {
            return Err(ApiError::Protocol(INVALID_METADATA.to_owned()));
        }
        for tag in &self.tags {
            if tag.trim().is_empty() || tag.chars().count() > 50 {
                return Err(ApiError::Protocol(INVALID_METADATA.to_owned()));
            }
        }
        match &self.geo {
            Some(geo)
                if !(-90.0..=90.0).contains(&geo.lat) || !(-180.0..=180.0).contains(&geo.lng) =>
            {
                return Err(ApiError::Protocol(INVALID_METADATA.to_owned()));
            }
            Some(_) | None => {}
        }
        Ok(())
    }
}

/// Vesper keeps a photo's date and place from the upload; only the caption and tags change.
#[derive(Deserialize)]
pub struct Update {
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemotePhoto {
    id: String,
    width: u32,
    height: u32,
    taken_at: Option<i64>,
    geo: Option<Geo>,
    entry_id: String,
    caption: String,
    tags: Vec<String>,
    occurred_at: i64,
    thumb_hash: String,
}

#[derive(Deserialize)]
struct RemotePage {
    photos: Vec<RemotePhoto>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct TagList {
    tags: Vec<TagCount>,
}

#[derive(Deserialize)]
struct TagCount {
    name: String,
}

#[derive(Deserialize)]
struct Created {
    id: String,
}

#[derive(Deserialize)]
struct EntryPhotos {
    photos: Vec<EntryPhoto>,
}

#[derive(Deserialize)]
struct EntryPhoto {
    id: String,
}

fn iso(milliseconds: i64) -> Option<String> {
    let time =
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(milliseconds) * 1_000_000)
            .ok()?;
    time.format(&time::format_description::well_known::Rfc3339)
        .ok()
}

fn milliseconds(value: &str) -> Option<i64> {
    let time =
        time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()?;
    i64::try_from(time.unix_timestamp_nanos() / 1_000_000).ok()
}

fn split_caption(caption: &str) -> (String, Option<String>) {
    match caption.split_once("\n\n") {
        Some((title, rest)) if !rest.trim().is_empty() => (title.to_owned(), Some(rest.to_owned())),
        Some((title, _)) => (title.to_owned(), None),
        None => (caption.to_owned(), None),
    }
}

fn caption(title: &str, description: Option<&str>) -> String {
    match description.map(str::trim).filter(|text| !text.is_empty()) {
        Some(description) => format!("{}\n\n{description}", title.trim()),
        None => title.trim().to_owned(),
    }
}

/// Vesper sends thumb hashes as base64url; the desktop decodes hex.
fn hex_thumb_hash(value: &str) -> Option<String> {
    URL_SAFE_NO_PAD.decode(value).ok().map(hex::encode)
}

impl RemotePhoto {
    fn into_photo(self) -> Photo {
        let (title, description) = split_caption(&self.caption);
        Photo {
            image_key: format!("{KEY_PREFIX}{}.jpg", self.id),
            thumbnail_key: format!("{KEY_PREFIX}{}.thumb.jpg", self.id),
            thumb_hash: hex_thumb_hash(&self.thumb_hash),
            date: iso(self.taken_at.unwrap_or(self.occurred_at)),
            id: self.id,
            entry_id: self.entry_id,
            title,
            description,
            width: self.width,
            height: self.height,
            tags: self.tags,
            geo: self.geo,
        }
    }
}

pub async fn list() -> Result<Page, ApiError> {
    let client = Client::load().await?;
    let mut photos = Vec::new();
    let mut before: Option<String> = None;
    loop {
        let mut request = client
            .http
            .get(format!("{VESPER_API}/photos"))
            .bearer_auth(&client.token);
        if let Some(cursor) = &before {
            request = request.query(&[("before", cursor)]);
        }
        let page: RemotePage = send(request, "list photos").await?.json().await?;
        photos.extend(page.photos.into_iter().map(RemotePhoto::into_photo));
        match page.next {
            Some(next) => before = Some(next),
            None => break,
        }
    }
    Ok(Page {
        total: photos.len(),
        photos,
    })
}

async fn get(id: &str) -> Result<Photo, ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::Protocol("a photo ID is required".to_owned()));
    }
    list()
        .await?
        .photos
        .into_iter()
        .find(|photo| photo.id == id)
        .ok_or_else(|| ApiError::Protocol(format!("photo {id} was not found")))
}

pub async fn tags() -> Result<Vec<String>, ApiError> {
    let client = Client::load().await?;
    let response = send(
        client
            .http
            .get(format!("{VESPER_API}/tags"))
            .bearer_auth(&client.token),
        "list photo tags",
    )
    .await?;
    let result: TagList = response.json().await?;
    Ok(result.tags.into_iter().map(|tag| tag.name).collect())
}

pub async fn upload(input: Upload, source: Vec<u8>) -> Result<Photo, ApiError> {
    input.validate()?;
    let prepare_task = tokio::task::spawn_blocking(move || media::prepare(&source))
        .await
        .map_err(|_| ApiError::Protocol("photo processor stopped unexpectedly".to_owned()))?;
    let prepared = prepare_task.map_err(ApiError::Media)?;
    let tags = input
        .tags
        .iter()
        .map(|tag| tag.trim().to_lowercase())
        .collect::<Vec<_>>();
    let digest = hex::encode(Sha256::digest(&prepared.original));
    let client = Client::load().await?;
    send(
        client
            .http
            .put(format!("{VESPER_API}/photos"))
            .bearer_auth(&client.token)
            .header(reqwest::header::CONTENT_TYPE, "image/png")
            .header("x-content-sha256", &digest)
            .body(prepared.original),
        "upload photo original",
    )
    .await?;
    let taken_at = input.date.as_deref().and_then(milliseconds);
    let thumb_hash = hex::decode(&prepared.thumb_hash)
        .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
        .map_err(|_| ApiError::Protocol("the photo thumb hash is invalid".to_owned()))?;
    let mut entry = serde_json::json!({
        "body": caption(&input.title, input.description.as_deref()),
        "tags": tags,
        "visibility": "public",
    });
    if let Some(taken_at) = taken_at {
        entry["occurredAt"] = serde_json::json!(taken_at);
    }
    let meta = serde_json::json!({
        "width": prepared.width,
        "height": prepared.height,
        "thumbHash": thumb_hash,
        "takenAt": taken_at,
        "geo": input.geo,
        "place": null,
        "exif": {
            "camera": null, "lens": null, "focalLength": null, "fNumber": null,
            "exposure": null, "iso": null, "film": null
        }
    });
    let form = reqwest::multipart::Form::new()
        .text("entry", entry.to_string())
        .text("digest", digest)
        .part(
            "thumb",
            reqwest::multipart::Part::bytes(prepared.thumbnail)
                .file_name("thumb.jpg")
                .mime_str("image/jpeg")?,
        )
        .text("mime", "image/png")
        .text("meta", meta.to_string());
    let created: Created = send(
        client
            .http
            .post(format!("{VESPER_API}/entries"))
            .bearer_auth(&client.token)
            .multipart(form),
        "create photo entry",
    )
    .await?
    .json()
    .await?;
    let entry: EntryPhotos = send(
        client
            .http
            .get(resource_url("entries", &created.id)?)
            .bearer_auth(&client.token),
        "read photo entry",
    )
    .await?
    .json()
    .await?;
    let photo = entry
        .photos
        .first()
        .ok_or_else(|| ApiError::Protocol("the new entry has no photo".to_owned()))?;
    get(&photo.id).await
}

pub async fn image(key: &str) -> Result<Vec<u8>, ApiError> {
    let name = key
        .strip_prefix(KEY_PREFIX)
        .ok_or_else(|| ApiError::Protocol("not a Vesper photo key".to_owned()))?;
    let (id, size) = match name.strip_suffix(".thumb.jpg") {
        Some(id) => (id, "thumb"),
        None => (
            name.strip_suffix(".jpg")
                .ok_or_else(|| ApiError::Protocol("not a Vesper photo key".to_owned()))?,
            "original",
        ),
    };
    let client = Client::load().await?;
    let mut url = resource_url("photos", id)?;
    url.query_pairs_mut().append_pair("size", size);
    let response = send(
        client.http.get(url).bearer_auth(&client.token),
        "read photo image",
    )
    .await?;
    Ok(response.bytes().await?.to_vec())
}

pub async fn update(id: &str, input: &Update) -> Result<Photo, ApiError> {
    let photo = get(id).await?;
    let client = Client::load().await?;
    let body = caption(&input.title, Some(&input.description));
    let tags = input
        .tags
        .iter()
        .map(|tag| tag.trim().to_lowercase())
        .collect::<Vec<_>>();
    send(
        client
            .http
            .patch(resource_url("entries", &photo.entry_id)?)
            .bearer_auth(&client.token)
            .json(&serde_json::json!({ "body": body, "tags": tags })),
        "update photo",
    )
    .await?;
    get(id).await
}

pub async fn delete(id: &str) -> Result<(), ApiError> {
    let photo = get(id).await?;
    let client = Client::load().await?;
    send(
        client
            .http
            .delete(resource_url("entries", &photo.entry_id)?)
            .bearer_auth(&client.token),
        "delete photo",
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Update, Upload};

    #[test]
    fn splits_and_joins_captions() {
        assert_eq!(
            super::split_caption("Title\n\nMore\n\nText"),
            ("Title".to_owned(), Some("More\n\nText".to_owned()))
        );
        assert_eq!(super::split_caption("Title"), ("Title".to_owned(), None));
        assert_eq!(super::caption("Title", Some("More")), "Title\n\nMore");
        assert_eq!(super::caption("Title", Some("  ")), "Title");
    }

    #[test]
    fn converts_thumb_hashes() {
        assert_eq!(super::hex_thumb_hash("AQID").as_deref(), Some("010203"));
    }

    #[test]
    fn caption_update() {
        let update: Update = serde_json::from_value(serde_json::json!({
            "title": " Harbour ",
            "description": "",
            "tags": ["Sea"]
        }))
        .expect("desktop update should match the command contract");

        assert_eq!(
            super::caption(&update.title, Some(&update.description)),
            "Harbour"
        );
        assert_eq!(update.tags, ["Sea"]);
    }

    #[test]
    fn decodes_upload() {
        let upload: Upload = serde_json::from_value(serde_json::json!({
            "title": "Shanghai",
            "description": "Evening",
            "tags": ["city"],
            "date": "2026-08-24T12:00:00.000Z",
            "geo": { "lat": 31.2304, "lng": 121.4737 }
        }))
        .expect("desktop upload should match the command contract");

        assert_eq!(upload.title, "Shanghai");
        assert_eq!(upload.tags, ["city"]);
        assert_eq!(upload.geo.expect("coordinates").lat, 31.2304);
    }
}
