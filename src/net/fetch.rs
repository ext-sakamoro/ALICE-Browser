use url::Url;

/// Result of fetching a URL
#[derive(Clone)]
pub struct FetchResult {
    pub html: String,
    pub url: String,
    pub status: u16,
    pub content_type: String,
}

/// Error during fetch
pub struct FetchError {
    pub message: String,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Fetch a URL and return the HTML content (blocking).
pub fn fetch_url(url_str: &str) -> Result<FetchResult, FetchError> {
    // Normalize URL
    let url = if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
        format!("https://{}", url_str)
    } else {
        url_str.to_string()
    };

    let parsed = Url::parse(&url).map_err(|e| FetchError {
        message: format!("Invalid URL: {}", e),
    })?;

    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!(
            "Mozilla/5.0 (compatible; ALICE-Browser/0.1; ",
            "+https://github.com/ext-sakamoro/ALICE-Browser)"
        ))
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| FetchError {
            message: format!("Client error: {}", e),
        })?;

    let response = client
        .get(parsed.as_str())
        .header(
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header("Accept-Language", "ja,en-US;q=0.9,en;q=0.8")
        .send()
        .map_err(|e| FetchError {
            message: format!("Request failed: {}", e),
        })?;

    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/html")
        .to_string();

    let final_url = response.url().to_string();

    let html = response.text().map_err(|e| FetchError {
        message: format!("Failed to read body: {}", e),
    })?;

    Ok(FetchResult {
        html,
        url: final_url,
        status,
        content_type,
    })
}
