# Cielo

Cielo builds a static weather application and its AEMET weather data ready to
serve as a static website.

## Build

Build the application shell with an explicit browser-facing data URL:

```sh
cielo build app --output-dir dist/app --data-url ../data/
```

The data URL may be relative to the deployed application, root-relative, or an
absolute HTTP(S) URL. Cielo treats it as a directory and adds a trailing slash
when needed.

Build the weather snapshot separately:

```sh
cielo build data --output-dir dist/data
```

`cielo build data` requires `AEMET_API_KEY` in the environment. The generated
data directory contains:

```text
municipalities.json
temperatures/<municipality-id>.json
```

The application and data output directories are independently managed artifact
roots. Each build replaces its complete output directory and refuses to replace
an unmanaged directory.
