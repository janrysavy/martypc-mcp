//! Lossless, explicit primitives for internal snapshot components.
//! No machine snapshot RPC is implied by these codecs.

pub(crate) fn required_option<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(d)
}

// Decimal float parsing can round differently. Clock/device state uses exact
// IEEE bits; this is state encoding, not a numeric JSON measurement field.
pub(crate) mod f64_bits {
    pub fn serialize<S: serde::Serializer>(value: &f64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(value.to_bits())
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        let bits: u64 = serde::Deserialize::deserialize(d)?;
        let value = f64::from_bits(bits);
        if !value.is_finite() {
            return Err(serde::de::Error::custom("nonfinite snapshot clock value"));
        }
        Ok(value)
    }
}
