use std::{fs, io, thread};
use std::collections::HashMap;
use std::error::Error;
use std::fs::{File, metadata, remove_file, rename};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{Datelike, DateTime, Days, NaiveDate, Utc};
use human_format::Formatter;
use plotters::prelude::*;
use rand::distributions::{Alphanumeric, DistString};
use regex::Regex;
use reqwest::{header, StatusCode};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use rusqlite::Connection;
use serde::Deserialize;

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

//
pub enum StatType {
    Clones,
    Views,
}

// Create a temporary file and move it to a target file
fn make_temp_file(target: PathBuf, b: &[u8]) -> io::Result<()> {
    let random_str = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);

    let tmpname = PathBuf::from(
        format!("cache/.tmp.{}.{}",
                random_str,
                target.extension().unwrap().to_str().expect("extension?")
        )
    );

    let mut f = File::create(&tmpname)?;
    f.write_all(b)?;
    f.flush()?;
    drop(f);

    rename(&tmpname, target)?;

    Ok(())
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

            make_temp_file(json_repos_fname, repos_json.as_bytes())?;
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

            make_temp_file(json_stats_fname, stats_json.as_bytes())?;
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

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(database_file: &PathBuf) -> Self {
        let conn = Connection::open(database_file)
            .expect("couldn't connect to local database");

        // See https://www.sqlite.org/lang_createtable.html
        conn.execute(r#"
          CREATE TABLE IF NOT EXISTS traffic (
            y INTEGER NOT NULL,
            m INTEGER NOT NULL,
            d INTEGER NOT NULL,

            owner TEXT NOT NULL,
            repo TEXT NOT NULL,

            c_count  INTEGER NOT NULL DEFAULT 0,
            c_uniq   INTEGER NOT NULL DEFAULT 0,

            v_count  INTEGER NOT NULL DEFAULT 0,
            v_uniq   INTEGER NOT NULL DEFAULT 0,

            PRIMARY KEY (y, m, d, owner, repo)
          )"#, (), // empty list of parameters.
        ).expect("couldn't create table: traffic");

        Self {
            conn: conn,
        }
    }

    // Update traffic stats
    pub fn update_traffic(
        &self,
        stat_type: StatType,
        owner: &str,
        repo: &str,
        stats: Vec<DayStats>,
    ) {
        for stat in stats {
            // See https://www.sqlite.org/lang_insert.html
            // Add empty row
            self.conn.execute(
                r#"INSERT OR IGNORE INTO
                     traffic
                     (y,  m,  d,  owner, repo) VALUES
                     (?1, ?2, ?3, ?4,    ?5)
                     "#,
                (
                    stat.timestamp.year(), stat.timestamp.month(), stat.timestamp.day(),
                    &owner,
                    &repo,
                ),
            ).expect("couldn't insert into traffic table");

            match stat_type {
                StatType::Clones => {
                    // https://www.sqlite.org/lang_update.html
                    self.conn.execute(
                        r#"UPDATE
                     traffic
                     SET
                       c_count=?6,
                       c_uniq=?7
                     WHERE
                       y=?1 AND m=?2 AND d=?3
                       AND owner=?4 AND repo=?5
                     "#,
                        (
                            stat.timestamp.year(),
                            stat.timestamp.month(),
                            stat.timestamp.day(),
                            &owner,
                            &repo,
                            stat.count,
                            stat.uniques,
                        ),
                    ).expect("couldn't update traffic table: clones");
                }
                StatType::Views => {
                    // https://www.sqlite.org/lang_update.html
                    self.conn.execute(
                        r#"UPDATE
                     traffic
                     SET
                       v_count=?6,
                       v_uniq=?7
                     WHERE
                       y=?1 AND m=?2 AND d=?3
                       AND owner=?4 AND repo=?5
                     "#,
                        (
                            stat.timestamp.year(),
                            stat.timestamp.month(),
                            stat.timestamp.day(),
                            &owner,
                            &repo,
                            stat.count,
                            stat.uniques,
                        ),
                    ).expect("couldn't update traffic table: views");
                }
            }
        }
    }

    // Get list of repositories
    pub fn get_repo_list(&self) -> rusqlite::Result<Vec<Repo>> {
        let mut stmt = self.conn.prepare(
            r#"SELECT
            owner, repo
            FROM traffic
            GROUP BY owner, repo
            ORDER BY owner, repo
            "#,
        )?;

        let mut res: Vec<Repo> = Vec::new();

        let items = stmt.query_map(
            [], |row| {
                Ok(Repo {
                    owner: row.get(0)?,
                    name: row.get(1)?,
                })
            })?;

        for item in items {
            res.push(item.unwrap());
        }

        Ok(res)
    }

    // Get traffic stats of a single repository
    pub fn get_repo_stats(
        &self,
        owner: &str,
        repo_name: &str,
    ) -> rusqlite::Result<Vec<RepoStats>> {
        let mut res: Vec<RepoStats> = Vec::new();

        let mut stmt = self.conn.prepare(
            r#"SELECT
              DATE(printf('%04d-%02d-%02d', y,m,d)) date,
              v_count, v_uniq,
              c_count, c_uniq
            FROM traffic
            WHERE
              owner=?1 AND repo=?2
            GROUP BY date
            ORDER BY date DESC
            LIMIT 30
            "#,
        )?;

        let items = stmt.query_map(
            (owner, repo_name), |row| {
                let date: NaiveDate = row.get(0)?;

                Ok(RepoStats {
                    date: date,
                    views: Stats {
                        count: row.get(1)?,
                        uniques: row.get(2)?,
                    },
                    clones: Stats {
                        count: row.get(3)?,
                        uniques: row.get(4)?,
                    },
                })
            })?;

        for item in items {
            res.push(item.unwrap());
        }

        Ok(res)
    }

    // Does given repository exist?
    pub fn repo_exists(
        &self,
        owner: &str,
        repo_name: &str,
    ) -> rusqlite::Result<bool> {
        let q = self.conn.query_row(
            r#"SELECT
              COUNT(repo) c
            FROM traffic
            WHERE
              owner=?1 AND repo=?2
            LIMIT 1
          "#,
            (owner, repo_name), |row| {
                let answer: u64 = row.get(0)?;
                Ok(answer)
            },
        )?;

        if q != 0 {
            return Ok(true);
        }

        Ok(false)
    }
}

pub struct Repo {
    pub owner: String,
    pub name: String,
}

pub struct Stats {
    pub count: u64,
    pub uniques: u64,
}

pub struct RepoStats {
    pub date: NaiveDate,
    pub views: Stats,
    pub clones: Stats,
}

pub struct ChartGenerator {
    data: HashMap<
        NaiveDate, HashMap<u8, u64>
    >,
    renames: HashMap<u8, String>,
    // For average(s)
    counts: HashMap<u8, u64>,
    width: u32,
    height: u32,
    filename: PathBuf,
    title: String,
}

impl ChartGenerator {
    pub fn new(
        title: String,
        filename: PathBuf,
        renames: HashMap<u8, String>,
    ) -> Self {
        Self {
            title: title,
            data: Default::default(),
            renames: renames,
            counts: Default::default(),
            width: 640,
            height: 480,
            filename: filename,
        }
    }

    // Add chart data points
    pub fn add(
        &mut self,
        d: NaiveDate,
        data: HashMap<u8, u64>,
    ) {
        for (k, v) in &data {
            *self.counts
                .entry(*k)
                .or_insert(0) += v;
        }

        // insert data
        self.data.insert(d, data);
    }

    // Render SVG
    pub fn render(&self) -> Result<(), Box<dyn Error>> {
        let mut max_y: u64 = 0;

        for (_, vals) in self.data.clone() {
            for (_, val) in vals {
                if val > max_y {
                    max_y = val;
                }
            }
        }

        if max_y < 10 {
            // Minimum 10, so that the zeroes don't go over the title
            max_y = 10;
        }

        let root = SVGBackend::new(
            self.filename.as_path(),
            (self.width, self.height),
        ).into_drawing_area();

        root.fill(&WHITE)?;
        let root = root.margin(5, 5, 20, 30);

        let now_naive = Utc::now().date_naive();

        // construct chart context
        let mut chart = ChartBuilder::on(&root)
            // Set the caption of the chart
            .caption(
                &self.title,
                ("sans-serif", 30).into_font(),
            )
            // Set the size of the label region
            .x_label_area_size(35)// days
            .y_label_area_size(30)// counts
            .build_cartesian_2d(
                0u32..30, // days 0-29 / 1-30
                0u64..((max_y + 9) / 10 * 10), // count of views / clones rounded to nearest ten
            )?
            ;


        // draw a mesh
        chart
            .configure_mesh()
            .x_desc(
                format!(
                    "Dates {:?} - {:?}",
                    now_naive,
                    now_naive.clone().checked_sub_days(Days::new(30)).expect("date error")
                )
            )
            .y_desc("Count")
            //.y_max_light_lines(1)
            // maximum number of labels allowed for each axis
            .x_labels(15)// days
            .y_labels(10)// counts

            // format of the label text
            .y_label_formatter(
                &|y| {
                    if *y < 10000 {
                        y.to_string()
                    } else {
                        Formatter::new()
                            .with_decimals(1)
                            .format(*y as f64)
                    }
                }
            )
            .x_label_formatter(
                &|x| {
                    format!(
                        "{:?}",
                        now_naive.checked_sub_days(
                            Days::new((*x) as u64)
                        ).expect("??")
                    )
                }
            )
            .draw()?;


        for typeid in 0u8..2 {
            let mut now = now_naive.clone().to_owned();
            let mut data: Vec<(u32, u64)> = vec![];

            // Last N days of data
            for day_index in 0..30 {
                match self.data.get(&now) {
                    None => { data.push((day_index, 0)) }
                    Some(d) => {
                        let val = match d.get(&typeid) {
                            None => { 0 }
                            Some(v) => { *v }
                        };

                        data.push((day_index, val));
                    }
                };

                now = match now.checked_sub_days(Days::new(1)) {
                    None => { panic!("invalid date"); }
                    Some(d) => { d }
                };
            }

            let color = Palette99::pick(typeid as usize).mix(0.9);

            // draw points
            chart
                .draw_series(
                    PointSeries::of_element(
                        data,
                        5,
                        color.clone().to_rgba(),
                        &|c, s, st| {
                            return EmptyElement::at(c)
                                + Circle::new((0, 0), s, st.filled()) // At this point, the new pixel coordinate is established
                                + Text::new(format!("{}", c.1), (-5, -18), ("sans-serif", 15).into_font());
                        },
                    )
                )?
                .label(
                    // Add legend name
                    match self.renames.get(&typeid) {
                        None => { String::from("?") }
                        Some(n) => {
                            // Add total counts
                            format!("{} ({})", n, self.counts[&typeid])
                        }
                    }
                )
                .legend(move |(x, y)|
                    Rectangle::new(
                        [(x - 10, y - 5), (x, y)],
                        color.clone().to_rgba().filled(),
                    )
                );
        } // /for

        // Legend
        chart.configure_series_labels()
            .position(SeriesLabelPosition::UpperRight)
            .margin(20)
            .legend_area_size(0)
            .border_style(BLUE)
            .background_style(BLUE.mix(0.1))
            .label_font(("sans-serif", 20))
            .draw()?
        ;

        root.present()?;

        Ok(())
    }

    // Reset internal data
    pub fn reset(&mut self) {
        self.data = Default::default();
        self.counts = Default::default();
    }
}
