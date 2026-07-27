# Cielo

Cielo is a static weather progressive web app (PWA) built using AEMET weather
forecast data.

The application and forecast data are built separately. Application assets only
need to be rebuilt when the app changes, while forecast data should be updated
periodically.

## Build

Run the build commands from the repository root. The data build fetches the
latest forecast from AEMET, so the `AEMET_API_KEY` environment variable must be
set first.

```sh
cargo run -- build data --output dist/data
```

```sh
cargo run -- build app --output dist/app --data ../data
```

The `--output` option is a filesystem path, while `--data` is the URL that the
browser uses to request the generated forecast data. In this layout, `app` and
`data` are sibling directories, so the app uses the relative URL `../data`.

Serve their common `dist` directory:

```sh
python3 -m http.server --directory dist 8000
```

Then open
[localhost:8000/app](http://localhost:8000/app/)

When the data is hosted separately, pass its public HTTP or HTTPS URL to
`--data` instead.

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
