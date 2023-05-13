use rusqlite::Connection;
use std::path::PathBuf;
use chrono::{Datelike, Days, NaiveDate};
use crate::github::DayStats;
use crate::{Repo, RepoStats, Stats, StatType};

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
        now_ref: NaiveDate,
        days: u32,
    ) -> rusqlite::Result<Vec<RepoStats>> {
        let mut res: Vec<RepoStats> = Vec::new();

        let mut stmt = self.conn.prepare(
            r#"SELECT
              DATE(printf('%04d-%02d-%02d', y,m,d)) date,
              v_count, v_uniq,
              c_count, c_uniq
            FROM traffic
            WHERE
              owner=?1 AND repo=?2 AND date >= DATE(?3)
            GROUP BY date
            ORDER BY date DESC
            LIMIT ?4
            "#,
        )?;

        // Calculate last date in range
        let days_ago = now_ref.checked_sub_days(
            Days::new(days as u64)
        ).unwrap();

        let items = stmt.query_map(
            (owner, repo_name, days_ago, days), |row| {
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
