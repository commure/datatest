//! Support module for `#[datatest::data(..)]`
use rustc_test::Bencher;
#[cfg(feature = "rustc_test_TDynBenchFn")]
use rustc_test::TDynBenchFn;
use serde::de::DeserializeOwned;
use std::path::Path;
use yaml_rust::parser::Event;
use yaml_rust::scanner::Marker;

/// Descriptor used internally for `#[datatest::data(..)]` tests.
#[doc(hidden)]
pub struct DataTestDesc {
    pub name: &'static str,
    pub ignore: bool,
    pub describefn: fn() -> Vec<DataTestCaseDesc<DataTestFn>>,
    pub source_file: &'static str,
}

/// Used internally for `#[datatest::data(..)]` tests.
#[doc(hidden)]
pub enum DataTestFn {
    TestFn(Box<dyn FnOnce() + Send + 'static>),

    #[cfg(feature = "rustc_test_TDynBenchFn")]
    BenchFn(Box<dyn TDynBenchFn + 'static>),

    #[cfg(not(feature = "rustc_test_TDynBenchFn"))]
    BenchFn(Box<dyn Fn(&mut Bencher) + Send + 'static>),
}

/// Descriptor of the data test case where the type of the test case data is `T`.
pub struct DataTestCaseDesc<T> {
    pub case: T,
    pub name: Option<String>,
    pub location: String,
}

pub fn yaml<T: DeserializeOwned + TestNameWithDefault + Send + 'static>(
    path: &str,
) -> Vec<DataTestCaseDesc<T>> {
    let input = std::fs::read_to_string(Path::new(path))
        .unwrap_or_else(|_| panic!("cannot read file '{}'", path));

    let index = index_cases(&input);
    let cases: Vec<T> = serde_yaml::from_str(&input).unwrap();
    assert_eq!(index.len(), cases.len(), "index does not match test cases");

    index
        .into_iter()
        .zip(cases)
        .map(|(marker, case)| DataTestCaseDesc {
            name: TestNameWithDefault::name(&case),
            case,
            location: format!("line {}", marker.line()),
        })
        .collect()
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
pub struct DataBenchFn<T>(pub fn(&mut Bencher, T), pub T)
where
    T: Send + Clone;

#[cfg(feature = "rustc_test_TDynBenchFn")]
impl<T> rustc_test::TDynBenchFn for DataBenchFn<T>
where
    T: Send + Clone,
{
    fn run(&self, harness: &mut Bencher) {
        (self.0)(harness, self.1.clone())
    }
}

impl<'r, T> Fn<(&'r mut Bencher,)> for DataBenchFn<T>
where
    T: Send + Clone,
{
    extern "rust-call" fn call(&self, (bencher,): (&'r mut Bencher,)) {
        (self.0)(bencher, self.1.clone());
    }
}

impl<'r, T> FnOnce<(&'r mut Bencher,)> for DataBenchFn<T>
where
    T: Send + Clone,
{
    type Output = ();
    extern "rust-call" fn call_once(self, harness: (&'r mut Bencher,)) {
        (self.0)(harness.0, self.1.clone())
    }
}

impl<'r, T> FnMut<(&'r mut Bencher,)> for DataBenchFn<T>
where
    T: Send + Clone,
{
    extern "rust-call" fn call_mut(&mut self, harness: (&'r mut Bencher,)) {
        (self.0)(harness.0, self.1.clone())
    }
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
