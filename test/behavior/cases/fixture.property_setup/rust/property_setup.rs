use std::collections::{BTreeMap, BTreeSet};

use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, SpecEvent)]
#[spec_component("fixture.property_setup")]
#[spec_event(setup)]
pub struct Query {
    pub filter: String,
    pub top: Option<u32>,
    pub tags: Vec<String>,
    pub allowed: BTreeSet<String>,
    pub weights: BTreeMap<String, Option<Vec<i32>>>,
}

#[derive(Debug, Default, SpecEvent)]
#[spec_component("fixture.property_setup")]
#[spec_event(setup, name = "RenamedSubset")]
pub struct Subset {
    #[spec_event(name = "limit")]
    pub selected: u32,
    pub cache: String,
}

#[derive(Debug, Default, SpecEvent)]
#[spec_component("fixture.property_setup")]
#[spec_event(setup)]
pub struct Empty {}

pub mod nested {
    #[derive(Debug, specgate::SpecEvent)]
    #[spec_component("fixture.property_setup")]
    #[spec_event(setup)]
    pub struct Fields {
        pub query: super::Query,
        pub subset: super::Subset,
        pub empty: super::Empty,
    }

    #[derive(Debug, specgate::SpecEvent)]
    #[spec_component("fixture.property_setup")]
    #[spec_event(setup)]
    pub struct Request {
        pub fields: self::Fields,
    }
}

#[spec_operation("nested_fields", spec = "fixture.property_setup")]
pub fn nested_fields(query: &mut self::nested::Request) -> String {
    let result = format!(
        "{}:{}",
        query.fields.query.filter, query.fields.subset.selected
    );
    query.fields.subset.selected += 1;
    result
}

#[spec_operation("zero_inputs", spec = "fixture.property_setup")]
pub fn zero_inputs(_query: Empty) -> i32 {
    3
}

#[spec_setup(
    "producer_precedence",
    fills = "query",
    spec = "fixture.property_setup"
)]
pub fn produce(filter: String) -> Query {
    Query {
        filter,
        top: None,
        tags: Vec::new(),
        allowed: BTreeSet::new(),
        weights: BTreeMap::new(),
    }
}

#[spec_operation("producer_precedence", spec = "fixture.property_setup")]
pub fn producer_precedence(query: Query) -> String {
    query.filter
}

#[derive(Debug, SpecEvent)]
#[spec_component("fixture.property_setup")]
pub struct Constructed {
    #[spec_event]
    pub filter: String,
}

#[spec_setup("constructor", fills = "query", spec = "fixture.property_setup")]
pub fn construct(filter: String) -> Constructed {
    Constructed { filter }
}

#[spec_operation("constructor", spec = "fixture.property_setup")]
pub fn constructor(query: Constructed) -> String {
    query.filter
}

#[spec_setup("nested_constructor", fills = "query", spec = "fixture.property_setup")]
pub fn construct_nested(options: self::nested::Request) -> Constructed {
    Constructed {
        filter: format!(
            "{}:{}",
            options.fields.query.filter, options.fields.subset.selected
        ),
    }
}

#[spec_operation("nested_constructor", spec = "fixture.property_setup")]
pub fn nested_constructor(query: Constructed) -> String {
    query.filter
}

#[derive(Debug, SpecEvent)]
#[spec_component("fixture.property_setup")]
pub struct NestedCounter {
    #[spec_event]
    pub count: u32,
}

#[spec_setup("nested_increment", spec = "fixture.property_setup")]
pub fn nested_counter(options: &mut self::nested::Request) -> NestedCounter {
    let count = options.fields.subset.selected;
    options.fields.subset.selected += 1;
    NestedCounter { count }
}

impl NestedCounter {
    #[spec_operation("nested_increment", spec = "fixture.property_setup")]
    pub fn increment(&mut self) -> u32 {
        self.count += 1;
        self.count
    }
}

#[spec_operation("all_fields", spec = "fixture.property_setup")]
pub fn all_fields(query: self::Query) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        query.filter,
        query.top.unwrap_or_default(),
        query.tags.join(","),
        query.allowed.into_iter().collect::<Vec<_>>().join(","),
        query.weights.values().flatten().flatten().sum::<i32>()
    )
}

#[spec_operation("subset_renamed", spec = "fixture.property_setup")]
pub fn subset_renamed(query: &mut self::Subset, selected: u32) -> u32 {
    let result = query.selected + selected;
    query.selected += 1;
    result
}

pub mod relative {
    pub mod operations {
        #[specgate::spec_operation("module_relative", spec = "fixture.property_setup")]
        pub fn read(query: &super::super::Query) -> String {
            query.filter.clone()
        }
    }
}

#[test]
fn constructor_and_property_inputs_are_behavioral() {
    assert_eq!(constructor(construct("created".to_string())), "created");
    assert_eq!(zero_inputs(Empty::default()), 3);
    assert_eq!(
        producer_precedence(produce("producer".to_string())),
        "producer"
    );
    let query = Query {
        filter: "x".to_string(),
        top: Some(10),
        tags: vec!["a".to_string(), "b".to_string()],
        allowed: BTreeSet::from(["b".to_string(), "a".to_string()]),
        weights: BTreeMap::from([
            ("some".to_string(), Some(vec![2, 3])),
            ("none".to_string(), None),
        ]),
    };
    assert_eq!(relative::operations::read(&query), "x");
    assert_eq!(all_fields(query), "x:10:a,b:a,b:5");
    assert_eq!(
        all_fields(Query {
            filter: String::new(),
            top: None,
            tags: Vec::new(),
            allowed: BTreeSet::new(),
            weights: BTreeMap::new(),
        }),
        ":0:::0"
    );
    let mut subset = Subset {
        selected: 7,
        cache: "not part of the contract".to_string(),
    };
    assert_eq!(subset_renamed(&mut subset, 2), 9);
    assert_eq!(subset_renamed(&mut subset, 2), 10);
    assert_eq!(subset.selected, 9);
    assert!(!subset.cache.is_empty());
    let mut nested = nested::Request {
        fields: nested::Fields {
            query: Query {
                filter: "nested".into(),
                top: Some(4),
                tags: vec!["tag".into()],
                allowed: BTreeSet::from(["b".into(), "a".into()]),
                weights: BTreeMap::new(),
            },
            subset: Subset {
                selected: 11,
                cache: "not reconstructed".into(),
            },
            empty: Empty {},
        },
    };
    assert_eq!(nested_fields(&mut nested), "nested:11");
    assert_eq!(nested_fields(&mut nested), "nested:12");
    let mut counter = nested_counter(&mut nested);
    assert_eq!(counter.increment(), 14);
    assert_eq!(counter.increment(), 15);
    assert_eq!(nested_constructor(construct_nested(nested)), "nested:14");
}
