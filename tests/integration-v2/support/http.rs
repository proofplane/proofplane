use url::Url;

/// Public grant URLs use the configured API base, not the test server address.
pub fn local_path(value: &str) -> String {
    let url = Url::parse(value).expect("grant URL parses");

    match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    }
}

pub fn request_cookie(cookie: &str) -> String {
    cookie
        .split(';')
        .next()
        .expect("cookie carries a name and value")
        .to_owned()
}
