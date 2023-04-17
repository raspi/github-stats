# github-stats

Generate GitHub traffic statistics SVG charts.

Traffic statistics for views and clones are downloaded from GitHub API to a local SQLite database.

GitHub keeps these statistics only for 14 days. With this program you can keep infinite days in local database and update the statistics once per day.

# Setting up

First copy `config.example.toml` to `config.toml`.
Generate or use existing GitHub API key.

# Example usage:

Fetch latest statistics:

```shell
github-stats fetch
```

Data from GitHub API is cached for one hour.

Generate SVG for a repository named *heksa*:

```shell
github-stats stats heksa
```

The generated chart is saved to `stats` directory.