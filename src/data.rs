//! Support module for `#[datatest::data(..)]`
use serde::de::DeserializeOwned;
use std::boxed::FnBox;
use yaml_rust::parser::Event;
use yaml_rust::scanner::Marker;

/// Descriptor used internally for `#[datatest::files(..)]` tests.
#[doc(hidden)]
pub struct DataTestDesc {
    pub name: &'static str,
    pub ignore: bool,
    pub root: &'static str,
    pub describefn: fn(&str) -> Vec<DataTestCase>,
}

#[doc(hidden)]
pub struct DataTestCase {
    pub name: Option<String>,
    pub line: usize,
    pub testfn: Box<FnBox() + Send + 'static>,
}

/// Trait abstracting two scenarios: test case implementing [`ToString`] and test case not
/// implementing [`ToString`].
#[doc(hidden)]
pub trait TestNameWithDefault {
    fn name(&self) -> Option<String>;
}

// For those types which do not implement `ToString`/`Display`.
impl<T> TestNameWithDefault for T {
    default fn name(&self) -> Option<String> {
        None
    }
}

// For those types which implement `ToString`/`Display`.
impl<T: ToString> TestNameWithDefault for T {
    fn name(&self) -> Option<String> {
        Some(self.to_string())
    }
}

/// Trait used to abstract two cases: `fn` taking test case as a value and `fn` taking test case
/// as a reference.
#[doc(hidden)]
pub trait TestFunc<T> {
    fn invoke(&self, value: T);
}

impl<T> TestFunc<T> for fn(T) {
    fn invoke(&self, value: T) {
        self(value)
    }
}

impl<T> TestFunc<T> for fn(&T) {
    fn invoke(&self, value: T) {
        self(&value)
    }
}

#[doc(hidden)]
pub fn describe<T, F>(source: &str, testfn: F) -> Vec<DataTestCase>
where
    T: DeserializeOwned + TestNameWithDefault + Send + 'static,
    F: TestFunc<T> + Send + Copy + 'static,
{
    let index = index_cases(source);
    let cases: Vec<T> = serde_yaml::from_str(source).unwrap();
    assert_eq!(index.len(), cases.len(), "index does not match test cases");

    // FIXME: crash if nothing is found!
    cases
        .into_iter()
        .enumerate()
        .map(|(idx, input)| DataTestCase {
            name: TestNameWithDefault::name(&input),
            line: index[idx].line(),
            testfn: Box::new(move || testfn.invoke(input)),
        })
        .collect()
}

/// Build an index from the YAML source to the location of each test case (top level array elements).
fn index_cases(source: &str) -> Vec<Marker> {
    let mut parser = yaml_rust::parser::Parser::new(source.chars());
    let mut index = Vec::new();
    let mut depth = 0;
    loop {
        let (event, marker) = parser.next().expect("invalid YAML");
        match event {
            Event::StreamEnd => {
                break;
            }
            Event::MappingStart(_idx) if depth == 1 => {
                index.push(marker);
                depth += 1;
            }
            Event::MappingStart(_idx) | Event::SequenceStart(_idx) => {
                depth += 1;
            }
            Event::MappingEnd | Event::SequenceEnd => {
                depth -= 1;
            }
            _ => {}
        }
    }

    index
}
