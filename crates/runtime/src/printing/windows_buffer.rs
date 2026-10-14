use super::{PrintError, PrintResult};

pub(super) enum BufferRead {
    Complete(usize),
    Resize(usize),
    Failed(String),
}

/// Read Win32 variable-length structures with explicit byte capacity, allowing data to grow during query.
pub(super) fn read_buffer(
    command: &str,
    mut query: impl FnMut(&mut [u64]) -> BufferRead,
) -> PrintResult<(Vec<u64>, usize)> {
    let mut buffer = Vec::new();
    for _ in 0..4 {
        match query(&mut buffer) {
            BufferRead::Complete(count) => return Ok((buffer, count)),
            BufferRead::Resize(bytes)
                if bytes > buffer.len() * 8 && bytes <= u32::MAX as usize - 7 =>
            {
                buffer.resize(bytes.div_ceil(8), 0);
            }
            BufferRead::Resize(_) => break,
            BufferRead::Failed(message) => {
                return Err(PrintError::CommandFailed {
                    command: command.to_string(),
                    message,
                });
            }
        }
    }
    Err(PrintError::CommandFailed {
        command: command.to_string(),
        message: "printer data changed repeatedly while reading".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{read_buffer, BufferRead};

    #[test]
    fn retries_with_larger_capacity_when_printer_data_grows() {
        let mut capacities = Vec::new();
        let (buffer, count) = read_buffer("EnumFormsW", |buffer| {
            capacities.push(buffer.len() * 8);
            match capacities.len() {
                1 => BufferRead::Resize(9),
                2 => BufferRead::Resize(25),
                _ => {
                    buffer[3] = 42;
                    BufferRead::Complete(2)
                }
            }
        })
        .unwrap();
        assert_eq!(capacities, [0, 16, 32]);
        assert_eq!(buffer[3], 42);
        assert_eq!(count, 2);
    }

    #[test]
    fn rejects_failed_or_repeatedly_changing_reads() {
        assert!(read_buffer("EnumFormsW", |_| BufferRead::Failed("driver error".into())).is_err());
        assert!(read_buffer("EnumFormsW", |buffer| {
            BufferRead::Resize(buffer.len() * 8 + 8)
        })
        .is_err());
    }
}
