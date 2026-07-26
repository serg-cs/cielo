# Cielo

Cielo generates a static weather using AEMET forecast data.

## Build

Building generates a ready to serve static website. Both app and data parts are
necessary but they are intentionally separate since data requires periodic
updating.

```sh
cargo run -- build app --output dist/app --data ../data/
```

```sh
cargo run -- build data --output dist/data
```

## Deploy

Deployment is set to allow directly sending built directories to an S3
compatible bucket.

Environment variables `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` are used
for deployment. An `--endpoint` and `--region` parameter are also supported.

```sh
cargo run -- deploy app --input dist/app --bucket cielo-app --region auto
cargo run -- deploy data --input dist/data --bucket cielo-data --region auto
```

Deployments overwrite objects with matching names and keep all other objects.
App assets are uploaded before the root `index.html` preventing the website
from being broken during deployment due to the use of hashed file names.
