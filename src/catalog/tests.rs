use super::{
    Beach, Catalog, Municipality, csv,
    update::{display_name, merge},
};

const PROVINCES: &str = "code,name\n03,Alicante\n28,Madrid\n35,Las Palmas\n";
const MUNICIPALITIES: &str = "id,name,latitude,longitude,altitude,population,warning_zone\n\
                              03014,Alicante,38.34500,-0.48300,3,337304,770301\n\
                              28079,Madrid,40.41680,-3.70380,657,3332035,722802\n\
                              35016,Las Palmas de Gran Canaria,28.10000,-15.41000,8,378797,659602\n";
const BEACHES: &str = "id,name,municipality,latitude,longitude\n0301401,\"Sant Joan, San Juan\",03014,38.38,-0.40889\n";

fn sample() -> Catalog {
    Catalog::parse(PROVINCES, MUNICIPALITIES, BEACHES).expect("sample catalog should parse")
}

#[test]
fn the_embedded_catalog_covers_spain() {
    let catalog = Catalog::embedded().expect("embedded catalog should parse");
    assert_eq!(catalog.provinces.len(), 52);
    assert!(catalog.municipalities.len() > 8_000);
    assert!(catalog.beaches.len() > 500);
    // Curated Spanish names, not AEMET's inverted or co-official forms.
    assert_eq!(
        catalog.municipalities["35016"].name,
        "Las Palmas de Gran Canaria"
    );
    assert_eq!(catalog.municipalities["03014"].name, "Alicante");
    assert_eq!(catalog.provinces["07"], "Islas Baleares");
    let madrid = &catalog.municipalities["28079"];
    assert_eq!(madrid.warning_zone.as_deref(), Some("722802"));
    assert!(madrid.latitude.is_some() && madrid.population.is_some());
}

#[test]
fn reads_quoted_fields() {
    let catalog = sample();
    assert_eq!(catalog.beaches[0].name, "Sant Joan, San Juan");
    assert_eq!(catalog.municipalities["28079"].altitude_metres, Some(657));
}

#[test]
fn rejects_inconsistent_catalogs() {
    let municipalities =
        |row: &str| format!("id,name,latitude,longitude,altitude,population,warning_zone\n{row}\n");
    for (municipalities, beaches) in [
        // Bad code, unknown province, alternative forms, out-of-range values.
        (municipalities("2807,Madrid,,,,,"), BEACHES.to_owned()),
        (municipalities("99001,Nowhere,,,,,"), BEACHES.to_owned()),
        (
            municipalities("03014,Alicante/Alacant,,,,,"),
            BEACHES.to_owned(),
        ),
        (
            municipalities("03014,Alicante,60.0,-0.4,,,"),
            BEACHES.to_owned(),
        ),
        (
            municipalities("03014,Alicante,,,,,7703"),
            BEACHES.to_owned(),
        ),
        // A beach in a municipality that is not listed.
        (
            municipalities("28079,Madrid,,,,,"),
            "id,name,municipality,latitude,longitude\n0301401,X,03014,38.3,-0.4\n".to_owned(),
        ),
        // A duplicate municipality.
        (
            MUNICIPALITIES.to_owned() + "28079,Madrid,,,,,\n",
            BEACHES.to_owned(),
        ),
    ] {
        assert!(
            Catalog::parse(PROVINCES, &municipalities, &beaches).is_err(),
            "should reject {municipalities} / {beaches}"
        );
    }
}

#[test]
fn writes_what_it_reads() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let catalog = sample();
    catalog.write(directory.path()).expect("write");
    let written = Catalog::read(directory.path()).expect("read back");
    assert_eq!(written.municipalities, catalog.municipalities);
    assert_eq!(written.beaches, catalog.beaches);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("beaches.csv")).expect("file"),
        BEACHES.replace("38.38,-0.40889", "38.38000,-0.40889")
    );
}

#[test]
fn quotes_only_when_needed() {
    let text = csv::write(
        &["a", "b"],
        &[vec!["x".to_owned(), "say \"hi\", then".to_owned()]],
    );
    assert_eq!(text, "a,b\nx,\"say \"\"hi\"\", then\"\n");
    assert_eq!(
        csv::parse_line("x,\"say \"\"hi\"\", then\"").expect("parse")[1],
        "say \"hi\", then"
    );
    assert!(csv::parse_line("\"open").is_err());
}

#[test]
fn display_names_put_the_article_first_and_keep_one_form() {
    assert_eq!(
        display_name("Palmas de Gran Canaria, Las"),
        "Las Palmas de Gran Canaria"
    );
    assert_eq!(
        display_name("Hospitalet de Llobregat, L'"),
        "L'Hospitalet de Llobregat"
    );
    assert_eq!(display_name("Alicante/Alacant"), "Alicante");
    assert_eq!(display_name("Madrid"), "Madrid");
}

#[test]
fn updates_keep_curated_names_and_report_changes() {
    let current = sample();
    let aemet = |id: &str, name: &str| Municipality {
        id: id.to_owned(),
        name: name.to_owned(),
        latitude: Some(40.0),
        longitude: Some(-3.0),
        altitude_metres: Some(1),
        population: Some(2),
        warning_zone: Some("722899".to_owned()),
    };
    let master = vec![
        aemet("03014", "Alacant/Alicante"),
        aemet("28079", "Madrid"),
        aemet("28006", "Alcobendas"),
        aemet("28092", "Rozas de Madrid, Las"),
    ];
    let beaches = vec![
        Beach {
            id: "0301402".to_owned(),
            name: "Postiguet".to_owned(),
            municipality_id: "03014".to_owned(),
            latitude: 38.3,
            longitude: -0.4,
        },
        Beach {
            id: "4600101".to_owned(),
            name: "Somewhere".to_owned(),
            municipality_id: "46001".to_owned(),
            latitude: 39.0,
            longitude: -0.3,
        },
    ];

    let (catalog, changes) = merge(&current, master, beaches);

    // AEMET's values replace the old ones; curated names stay.
    assert_eq!(catalog.municipalities["03014"].name, "Alicante");
    assert_eq!(
        catalog.municipalities["03014"].warning_zone.as_deref(),
        Some("722899")
    );
    assert_eq!(catalog.municipalities["28092"].name, "Las Rozas de Madrid");
    assert_eq!(
        changes.added,
        [
            (
                "28006".to_owned(),
                "Alcobendas".to_owned(),
                "Alcobendas".to_owned()
            ),
            (
                "28092".to_owned(),
                "Las Rozas de Madrid".to_owned(),
                "Rozas de Madrid, Las".to_owned()
            ),
        ]
    );
    assert_eq!(
        changes.removed,
        [("35016".to_owned(), "Las Palmas de Gran Canaria".to_owned())]
    );
    assert_eq!(changes.beaches_added, ["0301402"]);
    assert_eq!(changes.beaches_removed, ["0301401"]);
    assert_eq!(changes.beaches_skipped, ["4600101 Somewhere"]);
}
