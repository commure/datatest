//! Support module for `#[datatest::data(..)]`
use serde::de::DeserializeOwned;
use std::boxed::FnBox;
use test::TDynBenchFn;
use yaml_rust::parser::Event;
use yaml_rust::scanner::Marker;

/// Descriptor used internally for `#[datatest::data(..)]` tests.
#[doc(hidden)]
pub struct DataTestDesc {
    pub name: &'static str,
    pub ignore: bool,
    pub root: &'static str,
    pub describefn: fn(&str) -> Vec<DataTestCase>,
}

/// Used internally for `#[datatest::data(..)]` tests.
#[doc(hidden)]
pub enum DataTestFn {
    TestFn(Box<FnBox() + Send + 'static>),
    BenchFn(Box<TDynBenchFn + 'static>),
}

#[doc(hidden)]
pub struct DataTestCase {
    pub name: Option<String>,
    pub line: usize,
    pub testfn: DataTestFn,
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

#[doc(hidden)]
pub fn describe_test<T>(source: &str, testfn: fn(T)) -> Vec<DataTestCase>
where
    T: DeserializeOwned + TestNameWithDefault + Send + 'static,
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
            testfn: DataTestFn::TestFn(Box::new(move || testfn(input))),
        })
        .collect()
}

struct DataBenchFn<T>(fn(&mut test::Bencher, T), T)
where
    T: Send + Clone;

impl<T> test::TDynBenchFn for DataBenchFn<T>
where
    T: Send + Clone,
{
    fn run(&self, harness: &mut test::Bencher) {
        (self.0)(harness, self.1.clone())
    }
}

#[doc(hidden)]
pub fn describe_bench<T>(source: &str, benchfn: fn(&mut test::Bencher, T)) -> Vec<DataTestCase>
where
    T: DeserializeOwned + TestNameWithDefault + Clone + Send + 'static,
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
            testfn: DataTestFn::BenchFn(Box::new(DataBenchFn(benchfn, input))),
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
            Event::Scalar(_, _, _, _) if depth == 1 => {
                index.push(marker);
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
