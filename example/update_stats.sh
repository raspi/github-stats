#!/usr/bin/env bash

set -e

./github-stats fetch
./github-stats generate

cp -r stats/*.svg /domains/ghstats.mydomain.com

