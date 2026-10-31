# Catalog

The places Cielo knows. The files are embedded in the `cielo` binary, so a
data build never downloads them, and every build checks them: codes must be
well formed and unique, every municipality must belong to a province and every
beach to a municipality.

| File                 | Columns                                                                         | Source                                                                               |
| -------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `provinces.csv`      | `code`, `name`                                                                  | Curated by hand (52 provinces, INE codes).                                           |
| `municipalities.csv` | `id`, `name`, `latitude`, `longitude`, `altitude`, `population`, `warning_zone` | `name` is curated; the rest is AEMET's municipality list (`api/maestro/municipios`). |
| `beaches.csv`        | `id`, `name`, `municipality`, `latitude`, `longitude`                           | AEMET's beach list (`Playas_codigos.csv`), coordinates converted to decimal degrees. |

Data from [AEMET](https://www.aemet.es), © AEMET, reused under its
[terms](https://www.aemet.es/es/nota_legal).

## Names

Municipality names are the Spanish display names shown in the app. AEMET
writes many of them inverted (`Palmas de Gran Canaria, Las`) or with a
co-official form first (`Alacant/Alicante`), so names are kept here and never
taken from AEMET once a municipality is listed. A name has one form only: no
`/` alternatives.

## Updating

```sh
AEMET_API_KEY=… cielo catalog update
```

The command rewrites the three files with AEMET's current values, keeping the
curated names, and lists what changed:

- **New municipalities** get AEMET's name with the article moved to the front
  and only the first form kept. Check each one and fix it by hand if needed.
- **Removed municipalities** (merged or renamed by INE) are dropped.
- **Beaches** are replaced by AEMET's list; beaches whose municipality is not
  listed are skipped and reported.

Review the diff before committing. Run it a few times a year, or when AEMET
announces municipality changes.
