//! Multipart form data parsing.

use bytes::Bytes;
use futures_util::stream;
use multer::Multipart;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::types::UploadedFile;

/// Maximum upload size (10 MB)
const MAX_UPLOAD_SIZE: u64 = 10 * 1024 * 1024;

/// Parse multipart form data.
///
/// Returns a tuple of (form fields, uploaded files).
pub async fn parse_multipart(
    content_type: &str,
    body: Bytes,
) -> Result<(Vec<(String, String)>, Vec<(String, Vec<UploadedFile>)>), String> {
    let boundary = content_type
        .split(';')
        .find_map(|part| {
            let part = part.trim();
            if part.starts_with("boundary=") {
                Some(part[9..].trim_matches('"').to_string())
            } else {
                None
            }
        })
        .ok_or("Missing boundary in multipart content-type")?;

    let mut multipart =
        Multipart::new(stream::once(async { Ok::<_, std::io::Error>(body) }), boundary);

    let mut params = Vec::new();
    let mut files: Vec<(String, Vec<UploadedFile>)> = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| e.to_string())? {
        let field_name = field.name().unwrap_or("").to_string();
        let file_name = field.file_name().map(|s| s.to_string());
        let field_content_type = field
            .content_type()
            .map(|m| m.to_string())
            .unwrap_or_default();

        if let Some(original_name) = file_name {
            if original_name.is_empty() {
                continue;
            }

            let data = field.bytes().await.map_err(|e| e.to_string())?;
            let size = data.len() as u64;

            let normalized_name = if field_name.ends_with("[]") {
                field_name[..field_name.len() - 2].to_string()
            } else {
                field_name
            };

            let uploaded_file = if size > MAX_UPLOAD_SIZE {
                UploadedFile {
                    name: original_name,
                    mime_type: field_content_type,
                    tmp_name: String::new(),
                    size,
                    error: 1,
                }
            } else {
                let tmp_name = format!("/tmp/php{}", Uuid::new_v4().simple());

                let mut file = File::create(&tmp_name).await.map_err(|e| e.to_string())?;
                file.write_all(&data).await.map_err(|e| e.to_string())?;
                file.flush().await.map_err(|e| e.to_string())?;

                UploadedFile {
                    name: original_name,
                    mime_type: field_content_type,
                    tmp_name,
                    size,
                    error: 0,
                }
            };

            // Find existing entry or create new one
            if let Some(entry) = files.iter_mut().find(|(name, _)| name == &normalized_name) {
                entry.1.push(uploaded_file);
            } else {
                files.push((normalized_name, vec![uploaded_file]));
            }
        } else {
            let value = field.text().await.map_err(|e| e.to_string())?;
            params.push((field_name, value));
        }
    }

    Ok((params, files))
}
