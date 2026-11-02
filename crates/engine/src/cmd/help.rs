//! Help → community and project links (the FigureIDE GitHub repository, releases, and issue tracker).
//! The commands return the URL; the frontend opens it.

use serde_json::{Value, json};

use super::*;

pub const GITHUB_URL: &str = "https://github.com/newkor3185-blip/R_vector_studio";
pub const RELEASES_URL: &str = "https://github.com/newkor3185-blip/R_vector_studio/releases";
pub const ISSUES_URL: &str = "https://github.com/newkor3185-blip/R_vector_studio/issues";

pub fn github_url() -> String {
    GITHUB_URL.to_string()
}

pub fn releases_url() -> String {
    RELEASES_URL.to_string()
}

pub fn issues_url() -> String {
    ISSUES_URL.to_string()
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "help.github", "FigureIDE on GitHub", ["Help"], None, "{} → {url} source code repository", always, |_, _| url(github_url())),
        cmd!(query "help.releases", "FigureIDE Releases", ["Help"], None, "{} → {url} releases and downloads", always, |_, _| url(releases_url())),
        cmd!(query "help.issues", "Report an Issue", ["Help"], None, "{} → {url} issue tracker", always, |_, _| url(issues_url())),
        cmd!(query "help.discord", "Join Our Discord", ["Help"], None, "{} → {url} (unavailable)", always, |_, _| {
            Err(EngineError::Other("Discord community is unavailable".into()))
        }),
        cmd!(query "help.website", "Website", ["Help"], None, "{} → {url} (unavailable)", always, |_, _| {
            Err(EngineError::Other("Website is unavailable".into()))
        }),
        cmd!(query "help.appPage", "App Page", ["Help"], None, "{} → {url} (unavailable)", always, |_, _| {
            Err(EngineError::Other("App page is unavailable".into()))
        }),
        cmd!(query "help.links", "Links", [], None, "{} → {github, releases, issues}", always, |_, _| {
            Ok(json!({ "github": github_url(), "releases": releases_url(), "issues": issues_url() }))
        }),
    ]
}

fn url(u: String) -> Result<Value> {
    Ok(json!({ "url": u }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        let mut s = Session::new();
        assert_eq!(s.execute("help.github", &json!({})).unwrap()["url"], GITHUB_URL);
        assert_eq!(s.execute("help.releases", &json!({})).unwrap()["url"], RELEASES_URL);
        assert_eq!(s.execute("help.issues", &json!({})).unwrap()["url"], ISSUES_URL);
        let links = s.execute("help.links", &json!({})).unwrap();
        assert_eq!(links["github"], GITHUB_URL);
        assert_eq!(links["releases"], RELEASES_URL);
        assert_eq!(links["issues"], ISSUES_URL);
        assert!(s.execute("help.discord", &json!({})).is_err());
        assert!(s.execute("help.website", &json!({})).is_err());
        assert!(s.execute("help.appPage", &json!({})).is_err());
    }
}
