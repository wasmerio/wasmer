use std::collections::HashMap;
use std::io::Read;

/// Represents a snapshot of parts of the store that mutate
/// (such as globals and tables)
#[derive(Debug, Default)]
pub struct StoreSnapshot
{
    /// Global values at the time the snapshot was taken
    pub globals: HashMap<u32, u128>,
}

impl StoreSnapshot
{
    /// Serializes the snapshot into a set of bytes
    pub fn serialize(&self) -> Vec<u8> {
        let capacity = 32usize * self.globals.len();
        let mut ret = Vec::with_capacity(capacity);

        ret.extend_from_slice(&1u32.to_le_bytes());
        ret.extend_from_slice(&(self.globals.len() as u32).to_le_bytes());
        for (index, val) in self.globals.iter() {
            ret.extend_from_slice(&index.to_le_bytes());
            ret.extend_from_slice(&val.to_le_bytes());
        }
        ret
    }
    
    /// Deserializes the bytes back into a store snapshot
    pub fn deserialize(data: &[u8]) -> std::io::Result<Self> {
        let mut ret = StoreSnapshot::default();

        // Read all the sections
        let mut reader = data;
        loop {
            let mut ty_arr = [0u8; 4];            
            if let Err(err) = reader.read_exact(&mut ty_arr) {
                if err.kind() == std::io::ErrorKind::UnexpectedEof {
                    break;
                }
                return Err(err);
            }

            let ty = u32::from_le_bytes(ty_arr);
            match ty {
                1u32 => {
                    // Read all the globals
                    let mut len_arr = [0u8; 4];
                    reader.read_exact(&mut len_arr)?;
                    let len = u32::from_le_bytes(len_arr) as usize;
                    for _ in 0..len {
                        // Read the key
                        let mut key_arr = [0u8; 4];
                        reader.read_exact(&mut key_arr)?;
                        let key = u32::from_le_bytes(key_arr);

                        // Read the value
                        let mut val_arr = [0u8; 16];
                        reader.read_exact(&mut val_arr)?;
                        let val = u128::from_le_bytes(val_arr);

                        // Set the value in the snapshot
                        ret.globals.insert(key, val);
                    }
                },
                _ => { break; }
            }
        }

        Ok(ret)
    }
}
