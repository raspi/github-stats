use std::{fs, io};
use std::collections::HashMap;
use std::error::Error;
use std::fs::rename;
use std::path::PathBuf;
use std::process::exit;

use chrono::{NaiveDate, Utc};
use clap::{Args, command, Parser, Subcommand};
use rand::distributions::{Alphanumeric, DistString};
use rand::random;
use serde::Deserialize;
use toml::from_str;

use githubstats::*;
use githubstats::StatType::{Clones, Views};

// Config file
#[derive(Deserialize)]
struct Config {
    // keys [database], [github], etc...
    database: ConfigDatabase,
    github: ConfigGitHub,
}

// Config file key: [github]
#[derive(Deserialize)]
struct ConfigGitHub {
    apikey: String,
    user: String,
}

// Config file key: [database]
#[derive(Deserialize)]
struct ConfigDatabase {
    filename: PathBuf, // SQLite database file name
}


// CLI arguments
// See: https://docs.rs/clap/latest/clap/
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[command(author, version, about, long_about = None, propagate_version = true)]
struct CLIArgs {
    #[clap(short = 'v', long, default_value = "false",
    help = "Be verbose?")]
    #[arg(global = true)]
    verbose: bool,

    #[clap(short = 'c', long, default_value = "config.toml",
    help = "Config file")]
    #[arg(global = true)]
    config: PathBuf,

    #[command(subcommand)]
    #[clap(help = "Command")]
    command: Commands,
}

// CLI subcommands
#[derive(Subcommand, Debug)]
enum Commands {
    #[clap(about = "Fetch traffic statistics from Github to a local database")]
    Fetch(CommandFetchArgs),

    #[clap(about = "List repositories found in local database")]
    ListRepos(CommandListReposArgs),

    #[clap(about = "Generate statistics for repo from local database")]
    Stats(CommandStatsArgs),

    #[clap(about = "Generate all statistics from local database")]
    Generate(CommandGenerateArgs),
}

#[derive(Args, Debug)]
struct CommandFetchArgs {}

#[derive(Args, Debug)]
struct CommandListReposArgs {}

#[derive(Args, Debug)]
struct CommandStatsArgs {
    #[clap(short = 'd', long, default_value = "30",
    help = "Days")]
    days: u32,

    #[clap(required = true,
    help = "Repository")]
    repo: String,
}

#[derive(Args, Debug)]
struct CommandGenerateArgs {
    #[clap(short = 'd', long, default_value = "30",
    help = "Days")]
    days: u32,
}


fn main() -> Result<(), io::Error> {
    let args: CLIArgs = CLIArgs::parse();

    if !args.config.exists() {
        eprintln!("couldn't find config file");
        exit(1)
    }

    if args.config.is_dir() {
        eprintln!("config must be file");
        exit(1)
    }

    let contents = fs::read_to_string(args.config)
        .expect("couldn't read config file");

    let config = match from_str::<Config>(&contents) {
        Ok(cfg) => { cfg }
        Err(e) => {
            eprintln!("config file error: {}", e);
            exit(1)
        }
    };
    drop(contents);

    // Get a static reference to current date so that
    // if user's stats generation might slip into next day
    // the generated date range remains the same
    let now_reference = Utc::now().date_naive();

    let db = Database::new(&config.database.filename);

    match args.command {
        Commands::Fetch(_) => {
            if config.github.user.is_empty() {
                eprintln!("no GitHub user in config file");
                exit(1)
            }

            if config.github.apikey.is_empty() {
                eprintln!("no GitHub API key in config file");
                exit(1)
            }

            let ghsc = GithubStats::new(&config.github.apikey);

            println!("Fetching repository list for https://github.com/{} ..", config.github.user);
            let repos: GhRepo = match ghsc.get_repositories(config.github.user) {
                Ok(r) => { r }
                Err(e) => {
                    eprintln!("{}", e);
                    exit(1);
                }
            };

            if repos.is_empty() {
                println!("No repositories found");
                exit(0)
            }

            for repo in repos {
                println!("Repo https://github.com/{} :", repo.full_name);

                // --- Clone stats
                let clone_stats = match ghsc.get_stats(Clones, &repo.owner.login, &repo.name) {
                    Ok(d) => { d }
                    Err(e) => {
                        eprintln!("error traffic clones: {}", e);
                        exit(1)
                    }
                };

                if !clone_stats.is_empty() {
                    println!("  Updating clones...");
                    db.update_traffic(Clones, &repo.owner.login, &repo.name, clone_stats);
                }

                // --- View stats
                let view_stats = match ghsc.get_stats(Views, &repo.owner.login, &repo.name) {
                    Ok(d) => { d }
                    Err(e) => {
                        eprintln!("error traffic views: {}", e);
                        exit(1)
                    }
                };

                if !view_stats.is_empty() {
                    println!("  Updating views...");
                    db.update_traffic(Views, &repo.owner.login, &repo.name, view_stats);
                }
            }

            println!("Database file {} updated.", config.database.filename.display());
        }

        // List repos found in database
        Commands::ListRepos(_) => {
            if !config.database.filename.exists() {
                eprintln!("missing database file");
                exit(1)
            }

            let repos = match db.get_repo_list() {
                Ok(r) => { r }
                Err(e) => {
                    eprintln!("error getting repo list: {}", e);
                    exit(1)
                }
            };

            let mut widths: Vec<usize> = Vec::new();
            let mut rows: Vec<Vec<String>> = Vec::new();

            for repo in repos {
                let mut row: Vec<String> = Vec::new();

                let url = format!("https://github.com/{}/{}", repo.owner, repo.name);

                row.push(repo.owner);
                row.push(repo.name);
                row.push(url);

                if widths.is_empty() {
                    // Initial widths
                    for i in 0..row.len() {
                        widths.push(row[i].len());
                    }
                } else {
                    // Update widths
                    for (i, rstr) in row.iter().enumerate() {
                        let l = rstr.len();
                        if l > widths[i] {
                            // Update width
                            widths[i] = l
                        }
                    }
                }

                rows.push(row);
            }

            for row in rows {
                println!("{0:1$} {2:>3$} {4:5$}",
                         row[0], widths[0],
                         row[1], widths[1],
                         row[2], widths[2],
                );
            }
        } // /Command

        // Generate statistics SVG
        Commands::Stats(subargs) => {
            if !config.database.filename.exists() {
                eprintln!("missing database file");
                exit(1)
            }

            if subargs.repo.is_empty() {
                eprintln!("No repo name provided");
                exit(1);
            }

            match generate(&db, config.github.user, subargs.repo.clone(), now_reference, subargs.days) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("error getting repo {} {}", &subargs.repo, e);
                    exit(1)
                }
            };
        } // /Command

        Commands::Generate(genargs) => {
            if !config.database.filename.exists() {
                eprintln!("missing database file");
                exit(1)
            }

            let repos = match db.get_repo_list() {
                Ok(r) => { r }
                Err(e) => {
                    eprintln!("error getting repo list: {}", e);
                    exit(1)
                }
            };

            for repo in repos {
                match generate(&db, config.github.user.clone(), repo.name.clone(), now_reference, genargs.days) {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("error getting repo {} {}", repo.name, e);
                        exit(1)
                    }
                };
            }
        } // /Command
    }

    // Ok
    Ok(())
}

// generate SVG chart for a repo
fn generate(
    db: &Database,
    owner: String,
    repo_name: String,
    now_ref: NaiveDate,
    days: u32,
) -> Result<(), Box<dyn Error>> {
    match db.repo_exists(&owner, &repo_name) {
        Ok(exists) => {
            if !exists {
                eprintln!("repo named {} doesn't exist in local database", &repo_name);
                exit(1);
            }
        }
        Err(e) => {
            eprintln!("error getting repo {} {}", &repo_name, e);
            exit(1)
        }
    }

    let stats = match db.get_repo_stats(&owner, &repo_name, now_ref, days) {
        Ok(r) => { r }
        Err(e) => {
            eprintln!("error getting repo {} {}", &repo_name, e);
            exit(1)
        }
    };

    let fpath = PathBuf::from("stats");
    if !fpath.exists() {
        fs::create_dir_all(fpath.clone())?;
    }

    for t in [Clones, Views] {
        let n = match t {
            Clones => "clones",
            Views => "views",
        };

        // Legend
        let renames: HashMap<u8, String> = [
            (0, "Count".to_string()),
            (1, "Unique".to_string()),
        ].iter().cloned().collect();

        let random_str = Alphanumeric.sample_string(&mut rand::thread_rng(), 16);

        let tmpfname = PathBuf::from("cache")
            .join(format!(".tmp-{}_{}_{}.svg", n, &repo_name, random_str))
            ;

        let fname = fpath
            .clone()
            .join(format!("{}_{}.svg", &repo_name, n))
            ;

        let mut chart_gen: ChartGenerator = ChartGenerator::new(
            format!("GitHub {} for {}", n, &repo_name),
            tmpfname.clone(),
            renames.clone(),
            days,
        );

        // Add clone and view count(s)
        for (_, item) in stats.iter().enumerate() {
            let mut m: HashMap<u8, u64> = HashMap::new();

            match t {
                Clones => {
                    m = [
                        (0, item.clones.count),
                        (1, item.clones.uniques),
                    ].iter().cloned().collect();
                }
                Views => {
                    m = [
                        (0, item.views.count),
                        (1, item.views.uniques),
                    ].iter().cloned().collect();
                }
            }

            chart_gen.add(item.date, m);
        } // /for

        // Render SVG
        match chart_gen.render() {
            Ok(_) => {
                println!(
                    "Generated {} temp statistics SVG for repo {} as {}",
                    n,
                    &repo_name,
                    tmpfname.clone().display(),
                );
            }
            Err(e) => {
                eprintln!("error generating {} SVG for repo {} {}", n, &repo_name, e);
                exit(1)
            }
        };

        match rename(tmpfname.clone(), fname.clone()) {
            Ok(_) => {
                println!(
                    "Moved {} statistics SVG for repo {} {} to {}",
                    n,
                    &repo_name,
                    tmpfname.clone().display(),
                    fname.clone().display(),
                );
            }
            Err(e) => {
                eprintln!("error moving {} to {}; {}", tmpfname.display(), fname.display(), e);
                exit(1)
            }
        };
    }

    Ok(())
}