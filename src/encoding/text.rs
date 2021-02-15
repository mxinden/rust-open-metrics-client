//! Open Metrics text format implementation.
//!
//! ```
//! # use open_metrics_client::encoding::text::encode;
//! # use open_metrics_client::metrics::counter::Counter;
//! # use open_metrics_client::registry::Registry;
//! # use std::sync::atomic::AtomicU64;
//! #
//! # // Create registry and counter and register the latter with the former.
//! # let mut registry = Registry::default();
//! # let counter = Counter::<AtomicU64>::new();
//! # registry.register(
//! #   "my_counter",
//! #   "This is my counter",
//! #   counter.clone(),
//! # );
//! # counter.inc();
//! let mut buffer = vec![];
//! encode(&mut buffer, &registry).unwrap();
//!
//! let expected = "# HELP my_counter This is my counter.\n".to_owned() +
//!                "# TYPE my_counter counter\n" +
//!                "my_counter_total 1\n" +
//!                "# EOF\n";
//! assert_eq!(expected, String::from_utf8(buffer).unwrap());
//! ```

use crate::metrics::counter::{self, Counter};
use crate::metrics::family::Family;
use crate::metrics::gauge::{self, Gauge};
use crate::metrics::histogram::Histogram;
use crate::metrics::{MetricType, TypedMetric};
use crate::registry::{Registry, Unit};

use generic_array::ArrayLength;
use std::io::Write;
use std::ops::Deref;

pub fn encode<W, M>(writer: &mut W, registry: &Registry<M>) -> Result<(), std::io::Error>
where
    W: Write,
    M: EncodeMetric,
{
    for (desc, metric) in registry.iter() {
        writer.write_all(b"# HELP ")?;
        writer.write_all(desc.name().as_bytes())?;
        if let Some(unit) = desc.unit() {
            writer.write_all(b"_")?;
            unit.encode(writer)?;
        }
        writer.write_all(b" ")?;
        writer.write_all(desc.help().as_bytes())?;
        writer.write_all(b"\n")?;

        writer.write_all(b"# TYPE ")?;
        writer.write_all(desc.name().as_bytes())?;
        if let Some(unit) = desc.unit() {
            writer.write_all(b"_")?;
            unit.encode(writer)?;
        }
        writer.write_all(b" ")?;
        metric.metric_type().encode(writer)?;
        writer.write_all(b"\n")?;

        if let Some(unit) = desc.unit() {
            writer.write_all(b"# UNIT ")?;
            writer.write_all(desc.name().as_bytes())?;
            writer.write_all(b"_")?;
            unit.encode(writer)?;
            writer.write_all(b" ")?;
            unit.encode(writer)?;
            writer.write_all(b"\n")?;
        }

        let encoder = Encoder {
            writer,
            name: &desc.name(),
            unit: desc.unit(),
            labels: None,
        };

        metric.encode(encoder)?;
    }

    writer.write_all(b"# EOF\n")?;

    Ok(())
}

// `Encoder` does not take a trait parameter for `writer` and `labels` because
// `EncodeMetric` which uses `Encoder` needs to be usable as a trait object in
// order to be able to register different metric types with a `Registry`. Trait
// objects can not use type parameters.
//
// TODO: Alternative solutions to the above are very much appreciated.
pub struct Encoder<'a, 'b> {
    writer: &'a mut dyn Write,
    name: &'a str,
    unit: &'a Option<Unit>,
    labels: Option<&'b dyn Encode>,
}

impl<'a, 'b> Encoder<'a, 'b> {
    pub fn encode_suffix(&mut self, suffix: &'static str) -> Result<BucketEncoder, std::io::Error> {
        self.write_name_and_unit()?;

        self.writer.write_all(b"_")?;
        self.writer.write_all(suffix.as_bytes()).map(|_| ())?;

        self.encode_labels()
    }

    pub fn no_suffix(&mut self) -> Result<BucketEncoder, std::io::Error> {
        self.write_name_and_unit()?;

        self.encode_labels()
    }

    fn write_name_and_unit(&mut self) -> Result<(), std::io::Error> {
        self.writer.write_all(self.name.as_bytes())?;
        if let Some(unit) = self.unit {
            self.writer.write_all(b"_")?;
            unit.encode(self.writer)?;
        }

        Ok(())
    }

    // TODO: Consider caching the encoded labels for Histograms as they stay the
    // same but are currently encoded multiple times.
    pub(self) fn encode_labels(&mut self) -> Result<BucketEncoder, std::io::Error> {
        if let Some(labels) = &self.labels {
            self.writer.write_all(b"{")?;
            labels.encode(self.writer)?;

            Ok(BucketEncoder {
                opened_curly_brackets: true,
                writer: self.writer,
            })
        } else {
            Ok(BucketEncoder {
                opened_curly_brackets: false,
                writer: self.writer,
            })
        }
    }

    pub fn with_label_set<'c, 'd>(&'c mut self, label_set: &'d dyn Encode) -> Encoder<'c, 'd> {
        debug_assert!(self.labels.is_none());

        Encoder {
            writer: self.writer,
            name: self.name,
            unit: self.unit,
            labels: Some(label_set),
        }
    }
}

#[must_use]
pub struct BucketEncoder<'a> {
    writer: &'a mut dyn Write,
    opened_curly_brackets: bool,
}

impl<'a> BucketEncoder<'a> {
    fn encode_bucket(&mut self, upper_bound: f64) -> Result<ValueEncoder, std::io::Error> {
        if self.opened_curly_brackets {
            self.writer.write_all(b", ")?;
        } else {
            self.writer.write_all(b"{")?;
        }

        self.writer.write_all(b"le=\"")?;
        if upper_bound == f64::MAX {
            self.writer.write_all(b"+Inf")?;
        } else {
            upper_bound.encode(self.writer)?;
        }
        self.writer.write_all(b"\"}")?;

        Ok(ValueEncoder {
            writer: self.writer,
        })
    }

    fn no_bucket(&mut self) -> Result<ValueEncoder, std::io::Error> {
        if self.opened_curly_brackets {
            self.writer.write_all(b"}")?;
        }
        Ok(ValueEncoder {
            writer: self.writer,
        })
    }
}

#[must_use]
pub struct ValueEncoder<'a> {
    writer: &'a mut dyn Write,
}

impl<'a> ValueEncoder<'a> {
    fn encode_value<V: Encode>(&mut self, v: V) -> Result<(), std::io::Error> {
        self.writer.write_all(b" ")?;
        v.encode(self.writer)?;
        self.writer.write_all(b"\n")?;
        Ok(())
    }
}

pub trait EncodeMetric {
    fn encode(&self, encoder: Encoder) -> Result<(), std::io::Error>;

    // One can not use [`TypedMetric`] directly, as associated constants are not
    // object safe and thus can not be used with dynamic dispatching.
    fn metric_type(&self) -> MetricType;
}

impl EncodeMetric for Box<dyn EncodeMetric> {
    fn encode(&self, encoder: Encoder) -> Result<(), std::io::Error> {
        self.deref().encode(encoder)
    }

    fn metric_type(&self) -> MetricType {
        self.deref().metric_type()
    }
}

pub trait SendEncodeMetric: EncodeMetric + Send {}

impl<T: EncodeMetric + Send> SendEncodeMetric for T {}

impl EncodeMetric for Box<dyn SendEncodeMetric> {
    fn encode(&self, encoder: Encoder) -> Result<(), std::io::Error> {
        self.deref().encode(encoder)
    }

    fn metric_type(&self) -> MetricType {
        self.deref().metric_type()
    }
}

pub trait Encode {
    fn encode(&self, writer: &mut dyn Write) -> Result<(), std::io::Error>;
}

impl Encode for f64 {
    fn encode(&self, mut writer: &mut dyn Write) -> Result<(), std::io::Error> {
        dtoa::write(&mut writer, *self)?;
        Ok(())
    }
}

impl Encode for u64 {
    fn encode(&self, mut writer: &mut dyn Write) -> Result<(), std::io::Error> {
        itoa::write(&mut writer, *self)?;
        Ok(())
    }
}

impl Encode for &str {
    fn encode(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        // TODO: Can we do better?
        writer.write_all(self.as_bytes())?;
        Ok(())
    }
}

impl Encode for Vec<(String, String)> {
    fn encode(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        if self.is_empty() {
            return Ok(());
        }

        let mut iter = self.iter().peekable();
        while let Some((name, value)) = iter.next() {
            writer.write_all(name.as_bytes())?;
            writer.write_all(b"=\"")?;
            writer.write_all(value.as_bytes())?;
            writer.write_all(b"\"")?;

            if iter.peek().is_some() {
                writer.write_all(b",")?;
            }
        }

        Ok(())
    }
}

impl Encode for MetricType {
    fn encode(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        let t = match self {
            MetricType::Counter => "counter",
            MetricType::Gauge => "gauge",
            MetricType::Histogram => "histogram",
            MetricType::Unknown => "unknown",
        };

        writer.write_all(t.as_bytes())?;
        Ok(())
    }
}

impl Encode for Unit {
    fn encode(&self, writer: &mut dyn Write) -> Result<(), std::io::Error> {
        let u = match self {
            Unit::Amperes => "amperes",
            Unit::Bytes => "bytes",
            Unit::Celsius => "celsius",
            Unit::Grams => "grams",
            Unit::Joules => "joules",
            Unit::Meters => "meters",
            Unit::Ratios => "ratios",
            Unit::Seconds => "seconds",
            Unit::Volts => "volts",
            Unit::Other(other) => other.as_str(),
        };

        writer.write_all(u.as_bytes())?;
        Ok(())
    }
}

impl<A> EncodeMetric for Counter<A>
where
    A: counter::Atomic,
    <A as counter::Atomic>::Number: Encode,
{
    fn encode(&self, mut encoder: Encoder) -> Result<(), std::io::Error> {
        encoder
            .encode_suffix("total")?
            .no_bucket()?
            .encode_value(self.get())?;

        Ok(())
    }

    fn metric_type(&self) -> MetricType {
        Self::TYPE
    }
}

impl<A> EncodeMetric for Gauge<A>
where
    A: gauge::Atomic,
    <A as gauge::Atomic>::Number: Encode,
{
    fn encode(&self, mut encoder: Encoder) -> Result<(), std::io::Error> {
        encoder.no_suffix()?.no_bucket()?.encode_value(self.get())?;

        Ok(())
    }
    fn metric_type(&self) -> MetricType {
        Self::TYPE
    }
}

impl<S, M> EncodeMetric for Family<S, M>
where
    S: Clone + std::hash::Hash + Eq + Encode,
    M: EncodeMetric + TypedMetric,
{
    fn encode(&self, mut encoder: Encoder) -> Result<(), std::io::Error> {
        let guard = self.read();
        for (label_set, m) in guard.iter() {
            let encoder = encoder.with_label_set(label_set);
            m.encode(encoder)?;
        }
        Ok(())
    }

    fn metric_type(&self) -> MetricType {
        M::TYPE
    }
}

impl<NumBuckets: ArrayLength<(f64, u64)>> EncodeMetric for Histogram<NumBuckets> {
    fn encode(&self, mut encoder: Encoder) -> Result<(), std::io::Error> {
        let (sum, count, buckets) = self.get();
        encoder
            .encode_suffix("sum")?
            .no_bucket()?
            .encode_value(sum)?;
        encoder
            .encode_suffix("count")?
            .no_bucket()?
            .encode_value(count)?;

        for (upper_bound, count) in buckets.iter() {
            encoder
                .encode_suffix("bucket")?
                .encode_bucket(*upper_bound)?
                .encode_value(*count)?;
        }

        Ok(())
    }

    fn metric_type(&self) -> MetricType {
        Self::TYPE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::counter::Counter;
    use crate::metrics::gauge::Gauge;
    use crate::metrics::histogram::exponential_series;
    use pyo3::{prelude::*, types::PyModule};
    use std::sync::atomic::AtomicU64;
    use generic_array::typenum::U10;

    #[test]
    fn encode_counter() {
        let mut registry = Registry::default();
        let counter = Counter::<AtomicU64>::new();
        registry.register("my_counter", "My counter", counter.clone());

        let mut encoded = Vec::new();

        encode(&mut encoded, &registry).unwrap();

        parse_with_python_client(String::from_utf8(encoded).unwrap());
    }

    #[test]
    fn encode_counter_with_unit() {
        let mut registry = Registry::default();
        let counter = Counter::<AtomicU64>::new();
        registry.register_with_unit("my_counter", "My counter", Unit::Seconds, counter.clone());

        let mut encoded = Vec::new();
        encode(&mut encoded, &registry).unwrap();

        let expected = "# HELP my_counter_seconds My counter.\n".to_owned()
            + "# TYPE my_counter_seconds counter\n"
            + "# UNIT my_counter_seconds seconds\n"
            + "my_counter_seconds_total 0\n"
            + "# EOF\n";
        assert_eq!(expected, String::from_utf8(encoded.clone()).unwrap());

        parse_with_python_client(String::from_utf8(encoded).unwrap());
    }

    #[test]
    fn encode_gauge() {
        let mut registry = Registry::default();
        let gauge = Gauge::<AtomicU64>::new();
        registry.register("my_gauge", "My gauge", gauge.clone());

        let mut encoded = Vec::new();

        encode(&mut encoded, &registry).unwrap();

        parse_with_python_client(String::from_utf8(encoded).unwrap());
    }

    #[test]
    fn encode_counter_family() {
        let mut registry = Registry::default();
        let family = Family::<Vec<(String, String)>, Counter<AtomicU64>>::default();
        registry.register("my_counter_family", "My counter family", family.clone());

        family
            .get_or_create(&vec![("method".to_string(), "GET".to_string())])
            .inc();

        let mut encoded = Vec::new();

        encode(&mut encoded, &registry).unwrap();

        parse_with_python_client(String::from_utf8(encoded).unwrap());
    }

    #[test]
    fn encode_histogram() {
        let mut registry = Registry::default();
        let histogram = Histogram::<U10>::new(exponential_series(1.0, 2.0));
        registry.register("my_histogram", "My histogram", histogram.clone());
        histogram.observe(1.0);

        let mut encoded = Vec::new();

        encode(&mut encoded, &registry).unwrap();

        parse_with_python_client(String::from_utf8(encoded).unwrap());
    }

    fn parse_with_python_client(input: String) {
        println!("{:?}", input);
        Python::with_gil(|py| {
            let parser = PyModule::from_code(
                py,
                r#"
from prometheus_client.openmetrics.parser import text_string_to_metric_families

def parse(input):
    families = text_string_to_metric_families(input)
    list(families)
"#,
                "parser.py",
                "parser",
            )
            .map_err(|e| e.to_string())
            .unwrap();
            parser
                .call1("parse", (input,))
                .map_err(|e| e.to_string())
                .unwrap();
        })
    }
}
