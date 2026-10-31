use serde::{Serialize, Serializer, ser::SerializeSeq as _};

pub(super) struct Sequence<Items>(pub Items);

impl<Items: Iterator + Clone> Serialize for Sequence<Items>
where
    Items::Item: Serialize,
{
    fn serialize<Writer>(&self, serializer: Writer) -> Result<Writer::Ok, Writer::Error>
    where
        Writer: Serializer,
    {
        let mut sequence = serializer.serialize_seq(None)?;
        for item in self.0.clone() {
            sequence.serialize_element(&item)?;
        }
        sequence.end()
    }
}
