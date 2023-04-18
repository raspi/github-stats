# Example daily chart update with systemd

1. Modify `update_stats.sh` to your liking.
2. Change paths to correct ones in `github-stats.service`

Enable and start the service for your user:

    % systemctl enable --user $(pwd)/github-stats.service
    % systemctl start --user github-stats.service

Enable the service timer for your user:

    % systemctl enable --user $(pwd)/github-stats.timer
    % systemctl start --user github-stats.timer

You should see the timer after first run with:

```shell
% systemctl list-timers --user github-stats
NEXT                         LEFT     LAST                         PASSED    UNIT               ACTIVATES           
Wed 2023-04-19 04:04:17 EEST 12h left Tue 2023-04-18 15:07:18 EEST 24min ago github-stats.timer github-stats.service
```

See logs with:

    % journalctl --user -eu github-stats.service
