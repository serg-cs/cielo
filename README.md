# Cielo

Cielo is a static weather progressive web app (PWA) built using AEMET weather
forecast data.

The application and forecast data are built separately. Application assets only
need to be rebuilt when the app changes, while forecast data should be updated
periodically.

## Build

Building generates a ready to serve static website.

```sh
cargo run -- build data --output dist/data
```

```sh
cargo run -- build app --output dist/app --data dist/data
```

The generated directories contain static files and can be served by any static
web server. For example:

```sh
python3 -m http.server --directory dist/app 8080
```

## Deploy

Built directories can be uploaded directly to an S3-compatible bucket.

Credentials are read from environment variables `AWS_ACCESS_KEY_ID` and
`AWS_SECRET_ACCESS_KEY` with support for custom endpoints and regions using
parameters `--endpoint` and `--region`.

```sh
cargo run -- deploy app --input dist/app --bucket cielo-app --region auto
cargo run -- deploy data --input dist/data --bucket cielo-data --region auto
```

Deployment replaces objects with matching names but does not remove other
objects already stored in the bucket.

App assets are uploaded before the root `index.html`, preventing the website
from being broken during deployment due to the use of hashed filenames.
