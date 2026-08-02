use super::{LocationNames, parse_catalog};

#[test]
fn catalogs_cover_the_complete_ine_2026_code_set() {
    let names = LocationNames::load().expect("embedded location catalogs should load");

    assert_eq!(names.municipalities.len(), 8_132);
    assert_eq!(names.provinces.len(), 52);
    for municipality_id in names.municipalities.keys() {
        assert!(
            names.provinces.contains_key(&municipality_id[..2]),
            "municipality {municipality_id} has an unknown province"
        );
    }
}

#[test]
fn resolves_established_spanish_location_names_by_ine_code() {
    let names = LocationNames::load().expect("embedded location catalogs should load");

    for (municipality_id, expected_municipality, expected_province) in [
        ("07026", "Ibiza", "Islas Baleares"),
        ("15030", "La Coruña", "La Coruña"),
        ("20036", "Fuenterrabía", "Guipúzcoa"),
        ("25120", "Lérida", "Lérida"),
        ("32054", "Orense", "Orense"),
        ("46145", "Játiva", "Valencia"),
        ("48044", "Guecho", "Vizcaya"),
    ] {
        assert_eq!(
            names
                .municipality(municipality_id)
                .expect("municipality should exist"),
            expected_municipality
        );
        assert_eq!(
            names
                .province(municipality_id)
                .expect("province should exist"),
            expected_province
        );
    }
}

#[test]
fn rejects_malformed_catalog_entries() {
    for source in [
        "",
        "identifier,name\n28079,Madrid\n",
        "code,name\n1,Madrid\n",
        "code,name\n28079 Madrid\n",
        "code,name\n28079,\n",
        "code,name\n28079, Madrid\n",
        "code,name\n28079,Madrid/Comunidad de Madrid\n",
        "code,name\n28079,Madrid\n28079,Villa de Madrid\n",
        "code,name\n28079,\"Madrid\n",
        "code,name\n28079,\"Madrid\" extra\n",
        "code,name\n28079,Madrid,España\n",
    ] {
        assert!(parse_catalog(source, 5, "municipality").is_err());
    }
}

#[test]
fn parses_quoted_csv_names() {
    let names = parse_catalog(
        "code,name\n17048,\"Castillo de Aro, Playa de Aro y S'Agaró\"\n",
        5,
        "municipality",
    )
    .expect("quoted CSV catalog should load");

    assert_eq!(
        names.get("17048").map(String::as_str),
        Some("Castillo de Aro, Playa de Aro y S'Agaró")
    );
}

#[test]
fn reports_codes_missing_from_the_catalog() {
    let names = LocationNames::load().expect("embedded location catalogs should load");

    assert!(names.municipality("35999").is_err());
    assert!(names.province("99999").is_err());
}
