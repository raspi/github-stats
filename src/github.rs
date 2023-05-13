use std::time::Duration;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{header, StatusCode};
use reqwest::blocking::Client;
use std::error::Error;
use std::path::PathBuf;
use std::{fs, thread};
use std::fs::{metadata, remove_file};
use std::collections::HashMap;
use regex::Regex;
use serde::Deserialize;
use chrono::{DateTime, Utc};
use crate::StatType;

mod github_date_format {
    use chrono::{DateTime, TimeZone, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    // "2023-03-26T00:00:00Z" (UTC)
    const FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

    pub fn serialize<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
    {
        let s = format!("{}", date.format(FORMAT));
        serializer.serialize_str(&s)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
        where
            D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Utc.datetime_from_str(&s, FORMAT)
            .map_err(serde::de::Error::custom)
    }
}

// Github API
#[derive(Deserialize)]
pub struct DayStats {
    #[serde(deserialize_with = "github_date_format::deserialize")]
    pub timestamp: DateTime<Utc>,
    pub count: u64,
    pub uniques: u64,
}

// Github API
// https://docs.github.com/en/rest/metrics/traffic?apiVersion=2022-11-28#get-repository-clones
#[derive(Deserialize)]
pub struct CloningStats {
    pub count: u64,
    pub uniques: u64,
    pub clones: Vec<DayStats>,
}

// Github API
// https://docs.github.com/en/rest/metrics/traffic?apiVersion=2022-11-28#get-page-views
#[derive(Deserialize)]
pub(crate) struct ViewStats {
    pub count: u64,
    pub uniques: u64,
    pub views: Vec<DayStats>,
}

pub type GhRepo = Vec<GhRepoElement>;

// Github API
#[derive(Debug, Deserialize)]
pub struct GhRepoElement {
    pub id: u64,
    pub node_id: String,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub owner: Owner,
    pub html_url: String,
    pub description: Option<String>,
    pub fork: bool,
    pub url: String,
    pub forks_url: String,
    pub keys_url: String,
    pub collaborators_url: String,
    pub teams_url: String,
    pub hooks_url: String,
    pub issue_events_url: String,
    pub events_url: String,
    pub assignees_url: String,
    pub branches_url: String,
    pub tags_url: String,
    pub blobs_url: String,
    pub git_tags_url: String,
    pub git_refs_url: String,
    pub trees_url: String,
    pub statuses_url: String,
    pub languages_url: String,
    pub stargazers_url: String,
    pub contributors_url: String,
    pub subscribers_url: String,
    pub subscription_url: String,
    pub commits_url: String,
    pub git_commits_url: String,
    pub comments_url: String,
    pub issue_comment_url: String,
    pub contents_url: String,
    pub compare_url: String,
    pub merges_url: String,
    pub archive_url: String,
    pub downloads_url: String,
    pub issues_url: String,
    pub pulls_url: String,
    pub milestones_url: String,
    pub notifications_url: String,
    pub labels_url: String,
    pub releases_url: String,
    pub deployments_url: String,

    #[serde(deserialize_with = "github_date_format::deserialize")]
    pub created_at: DateTime<Utc>,
    #[serde(deserialize_with = "github_date_format::deserialize")]
    pub updated_at: DateTime<Utc>,
    #[serde(deserialize_with = "github_date_format::deserialize")]
    pub pushed_at: DateTime<Utc>,

    pub git_url: String,
    pub ssh_url: String,
    pub clone_url: String,
    pub svn_url: String,
    pub homepage: Option<String>,
    pub size: u64,
    pub stargazers_count: u64,
    pub watchers_count: u64,
    pub language: Option<String>,
    pub has_issues: bool,
    pub has_projects: bool,
    pub has_downloads: bool,
    pub has_wiki: bool,
    pub has_pages: bool,
    pub has_discussions: bool,
    pub forks_count: i64,
    pub mirror_url: Option<String>,
    pub archived: bool,
    pub disabled: bool,
    pub open_issues_count: u64,
    pub license: Option<License>,
    pub allow_forking: bool,
    pub is_template: bool,
    pub web_commit_signoff_required: bool,
    pub topics: Vec<String>,
    pub visibility: String,
    pub forks: u64,
    pub open_issues: u64,
    pub watchers: u64,
    pub default_branch: String,
    pub permissions: Permissions,
}

// Github API
#[derive(Debug, Deserialize)]
pub struct License {
    pub key: String,
    pub name: String,
    pub spdx_id: String,
    pub url: Option<String>,
    pub node_id: String,
}

// Github API
#[derive(Debug, Deserialize)]
pub struct Owner {
    pub login: String,
    pub id: u64,
    pub node_id: String,
    pub avatar_url: String,
    pub gravatar_id: String,
    pub url: String,
    pub html_url: String,
    pub followers_url: String,
    pub following_url: String,
    pub gists_url: String,
    pub starred_url: String,
    pub subscriptions_url: String,
    pub organizations_url: String,
    pub repos_url: String,
    pub events_url: String,
    pub received_events_url: String,
    #[serde(rename = "type")]
    pub owner_type: String,
    pub site_admin: bool,
}

// Github API
#[derive(Debug, Deserialize)]
pub struct Permissions {
    pub admin: bool,
    pub maintain: bool,
    pub push: bool,
    pub triage: bool,
    pub pull: bool,
}

// HTTP API client for GitHub
#[derive(Clone)]
pub struct GithubStats {
    http_client: Client,
}

impl GithubStats {
    // Sleep time between HTTP requests
    // https://docs.github.com/en/rest/overview/resources-in-the-rest-api?apiVersion=2022-11-28#rate-limiting
    const RATE_LIMIT: Duration = Duration::from_millis(300);

    // HTTP client's timeout
    const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

    // JSON file cache duration
    const MAX_FILE_AGE: Duration = Duration::from_secs(60 * 60);

    pub fn new(
        api_key: &str, // GitHub API key
    ) -> Self {
        let mut headers = HeaderMap::new();

        let bearer = format!("Bearer {}", api_key);
        let auth_value = HeaderValue::from_str(bearer.as_str()).expect("");
        headers.insert(header::AUTHORIZATION, auth_value);

        headers.insert("Accept", header::HeaderValue::from_static("application/vnd.github+json"));
        headers.insert("X-GitHub-Api-Version", header::HeaderValue::from_static("2022-11-28"));

        let client = Client::builder()
            .user_agent("Github stats")
            .default_headers(headers)
            .timeout(Self::HTTP_TIMEOUT)
            .build()
            .unwrap();

        Self {
            http_client: client,
        }
    }

    // Get list of repositories
    pub fn get_repositories(
        &self,
        name: String,
    ) -> Result<GhRepo, Box<dyn Error>> {
        let mut l: GhRepo = GhRepo::new();

        let mut page_num = 1;

        loop {
            let (mut repo, has_next) = self.get_repos(name.clone(), page_num)?;
            l.append(&mut repo);

            if !has_next {
                break;
            }

            page_num += 1;
        }

        Ok(l)
    }

    // Get a single JSON page of repositories list
    fn get_repos(
        &self,
        name: String, // Repository's name
        page_num: u64,
    ) -> Result<(GhRepo, bool), Box<dyn Error>> {
        // How many repositories to list per JSON page
        const PER_PAGE: u16 = 100;
        let mut has_next = false;

        let cache_path = PathBuf::from(format!("cache/repos/{}", name));
        let mut json_repos_fname = PathBuf::from(cache_path.clone());
        json_repos_fname = json_repos_fname.join(format!("_REPOS_p{}.json", page_num));

        fs::create_dir_all(cache_path)?;

        let mut repos_json: String = String::new();

        if !json_repos_fname.exists() {
            // Do not flood Github API
            thread::sleep(Self::RATE_LIMIT);

            repos_json = match self.http_client.get(
                format!(
                    "https://api.github.com/users/{}/repos?type=all&sort=created&direction=asc&per_page={}&page={}",
                    name, PER_PAGE, page_num,
                )
            ).send() {
                Ok(r) => {
                    if r.status() == StatusCode::OK {
                        match r.headers().get("link") {
                            None => {}
                            Some(hv) => {
                                if !hv.is_empty() {
                                    let raw = hv.to_str()?;
                                    let link = Self::parse_links_header(raw);

                                    if link.contains_key("next") {
                                        // We have multiple pages of repos
                                        has_next = true;
                                    }
                                }
                            }
                        }

                        match r.text() {
                            Ok(d) => { d }
                            Err(e) => { Err(e.to_string())? }
                        }
                    } else {
                        Err(format!("status: {}", r.status()))?
                    }
                }
                Err(e) => { Err(e.to_string())? }
            };

            if repos_json.is_empty() {
                Err(format!("empty: {} (page {})", name, page_num))?
            }

            crate::make_temp_file(json_repos_fname, repos_json.as_bytes())?;
        } else {
            let md = metadata(json_repos_fname.clone())?;
            let file_age = md.created()?.elapsed()?;

            if file_age >= Self::MAX_FILE_AGE {
                // Too old, fetch again
                remove_file(json_repos_fname)?;
                return self.get_repos(name, page_num);
            }

            repos_json = fs::read_to_string(json_repos_fname)?;
        }

        if repos_json.is_empty() {
            Err(format!("empty: {} (page {})", name, page_num))?
        }

        match serde_json::from_str::<GhRepo>(&repos_json) {
            Ok(o) => { Ok((o, has_next)) }
            Err(e) => { Err(e.to_string())? }
        }
    }

    // Get traffic stats
    pub fn get_stats(
        &self,
        stat_type: StatType,
        owner: &str,
        repo_name: &str,
    ) -> Result<Vec<DayStats>, Box<dyn Error>> {
        let n = match stat_type {
            StatType::Clones => "clones",
            StatType::Views => "views",
        };

        let cache_path = PathBuf::from(format!("cache/repos/{}", owner));
        let mut json_stats_fname = PathBuf::from(cache_path.clone());
        json_stats_fname = json_stats_fname.join(format!("{}_{}.json", repo_name, n));

        fs::create_dir_all(cache_path).expect("couldn't create cache directory");

        let mut stats_json: String = String::new();

        if !json_stats_fname.exists() {
            // Do not flood Github API
            thread::sleep(Self::RATE_LIMIT);

            stats_json = match self.http_client
                .get(format!(
                    "https://api.github.com/repos/{}/{}/traffic/{}?per=day",
                    owner, repo_name, n
                ))
                .send()
            {
                Ok(r) => {
                    if r.status() == StatusCode::OK {
                        match r.text() {
                            Ok(d) => d,
                            Err(e) => { Err(e.to_string())? }
                        }
                    } else { Err(format!("status: {} ", r.status()))? }
                }
                Err(e) => { Err(e.to_string())? }
            };

            if stats_json.is_empty() {
                Err(format!("empty: {} {}/{}", n, owner, repo_name))?
            }

            crate::make_temp_file(json_stats_fname, stats_json.as_bytes())?;
        } else {
            let md = metadata(json_stats_fname.clone())?;
            let file_age = md.created()?.elapsed()?;

            if file_age >= Self::MAX_FILE_AGE {
                // Too old, fetch again
                remove_file(json_stats_fname)?;
                return self.get_stats(stat_type, owner, repo_name);
            }

            stats_json = fs::read_to_string(json_stats_fname)?;
        }

        if stats_json.is_empty() {
            Err(format!("empty: {} {}/{}", n, owner, repo_name))?
        }

        // Get daily stats, if any
        match stat_type {
            StatType::Clones => {
                match serde_json::from_str::<CloningStats>(&stats_json) {
                    Ok(o) => { Ok(o.clones) }
                    Err(e) => { Err(e.to_string())? }
                }
            }
            StatType::Views => {
                match serde_json::from_str::<ViewStats>(&stats_json) {
                    Ok(o) => { Ok(o.views) }
                    Err(e) => { Err(e.to_string())? }
                }
            }
        }
    }

    // parse "Link" header
    fn parse_links_header(raw_links: &str) -> HashMap<&str, &str> {
        let links_regex: Regex = Regex::new(
            r#"(<(?P<url>http(s)?://[^>\s]+)>; rel="(?P<rel>[[:word:]]+))+"#
        ).unwrap();

        links_regex
            .captures_iter(raw_links)
            .fold(HashMap::new(), |mut acc, cap| {
                let groups = (cap.name("url"), cap.name("rel"));
                match groups {
                    (Some(url), Some(rel)) => {
                        acc.insert(rel.as_str(), url.as_str());
                        acc
                    }
                    _ => acc,
                }
            })
    }
}
