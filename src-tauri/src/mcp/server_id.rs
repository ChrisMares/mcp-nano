use axum::extract::Request;
use axum::http::Uri;
use axum::middleware::Next;
use axum::response::Response;

/// Request extension carrying `?server_id=` from the MCP URL.
#[derive(Debug, Clone)]
pub struct ServerId(pub String);

pub async fn extract_server_id(mut req: Request, next: Next) -> Response {
    if let Some(id) = server_id_from_uri(req.uri()) {
        req.extensions_mut().insert(ServerId(id));
    }
    next.run(req).await
}

pub fn server_id_from_uri(uri: &Uri) -> Option<String> {
    let query = uri.query()?;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if key == "server_id" && !value.is_empty() {
            return Some(value.into_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_id_query() {
        let uri: Uri = "http://127.0.0.1:18651/mcp?server_id=abc-123"
            .parse()
            .unwrap();
        assert_eq!(server_id_from_uri(&uri).as_deref(), Some("abc-123"));
    }

    #[test]
    fn missing_server_id() {
        let uri: Uri = "http://127.0.0.1:18651/mcp".parse().unwrap();
        assert!(server_id_from_uri(&uri).is_none());
    }
}
