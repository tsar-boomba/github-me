#! /bin/sh
set -e
cargo lambda build --arm64 --release
rm -f ~/Downloads/github-me-api.zip
rm -f ~/Downloads/github-me-job.zip
cargo lambda deploy --dry --binary-name job
cargo lambda deploy --dry --binary-name api
cp target/lambda/api/bootstrap.zip ~/Downloads/github-me-api.zip
cp target/lambda/job/bootstrap.zip ~/Downloads/github-me-job.zip
