//! 静的ファイル配信 + リダイレクトレスポンス。

use axum::response::{Html, IntoResponse, Response};

use crate::web::Assets;

/// `rust_embed::Embed` から HTML を取得して返す
pub(in crate::web) fn serve_asset(name: &str) -> Response {
    match Assets::get(name) {
        Some(file) => {
            Html(String::from_utf8_lossy(file.data.as_ref()).into_owned()).into_response()
        }
        None => Response::builder()
            .status(404)
            .body(axum::body::Body::from(format!("{} not found", name)))
            .expect("Response::builder への無効な設定"),
    }
}

/// 302 リダイレクトレスポンスを生成する
pub(in crate::web) fn redirect(location: &str) -> Response {
    Response::builder()
        .status(302)
        .header("Location", location)
        .body(axum::body::Body::empty())
        .expect("Response::builder への無効なヘッダー値")
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
