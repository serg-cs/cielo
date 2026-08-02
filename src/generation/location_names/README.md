# Spanish location names

`municipalities.csv` contains one display-ready Spanish name for every municipality
in the INE list dated 1 January 2026. `provinces.csv` contains the corresponding
Spanish province names.

Municipality identity and coverage come from the INE workbook:

<https://www.ine.es/daco/daco42/codmun/diccionario26.xlsx>

Spanish municipality-name candidates come from Spanish-language Wikidata labels
joined through property P772, the INE municipality code. Structured Wikidata data
is available under CC0:

<https://www.wikidata.org/wiki/Wikidata:Licensing>

The INE data is reused under CC BY 4.0:

<https://www.ine.es/dyngs/AYU/es/index.htm?cid=125>

`overrides.csv` resolves codes that are absent or ambiguous in Wikidata. Province
names are curated because the INE publishes official registered forms rather than
Spanish display forms. Spanish forms such as `Lérida` and `Islas Baleares` are
expressly permitted for non-official Spanish-language use by Ley 2/1992 and Ley
13/1997.

To regenerate the municipality catalog:

```sh
python3 tools/update_location_names.py
cargo test
```

The updater fails on missing or ambiguous candidates so source changes must be
reviewed explicitly instead of silently reaching published weather data. Wikidata
lookups are limited to the active INE codes and sent in small sequential batches
to stay below the public query service's deadline and request-rate limits.
