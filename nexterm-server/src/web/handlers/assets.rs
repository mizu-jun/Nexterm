//! Static-file serving and redirect responses.

use axum::response::{Html, IntoResponse, Response};

use crate::web::Assets;

/// Fetch HTML from `rust_embed::Embed` and return it.
pub(in crate::web) fn serve_asset(name: &str) -> Response {
    match Assets::get(name) {
        Some(file) => {
            Html(String::from_utf8_lossy(file.data.as_ref()).into_owned()).into_response()
        }
        None => Response::builder()
            .status(404)
            .body(axum::body::Body::from(format!("{} not found", name)))
            .expect("invalid configuration for Response::builder"),
    }
}

/// Build a 302 redirect response.
pub(in crate::web) fn redirect(location: &str) -> Response {
    Response::builder()
        .status(302)
        .header("Location", location)
        .body(axum::body::Body::empty())
        .expect("invalid header value for Response::builder")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_creates_302_response() {
        let response = redirect("/login");
        assert_eq!(response.status(), 302);
        assert!(
            response
                .headers()
                .get("location")
                .unwrap()
                .to_str()
                .unwrap()
                .contains("/login")
        );
    }
}
