use anyhow::{Context, bail};
use std::io::{Read, Write};

use crate::result::Result;

/// Request from client to daemon to execute a command in the container
#[derive(Debug, Clone)]
pub(crate) struct ExecRequest {
    /// Command to execute (None means use default shell)
    pub command: Option<String>,
    /// Arguments to pass to the command
    pub arguments: Vec<String>,
}

/// Response from daemon to client after processing exec request
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecResponse {
    /// Exec request accepted, daemon will handle it
    Ok,
    /// Error occurred, contains error message
    Error(String),
}

impl ExecRequest {
    /// Create a new exec request
    pub fn new(command: Option<String>, arguments: Vec<String>) -> Self {
        ExecRequest { command, arguments }
    }

    /// Serialize the request to a byte stream
    ///
    /// Format:
    /// - 1 byte: has_command flag (0 = None, 1 = Some)
    /// - if has_command = 1:
    ///   - 4 bytes: command length (u32, little-endian)
    ///   - N bytes: command string (UTF-8)
    /// - 4 bytes: argument count (u32, little-endian)
    /// - for each argument:
    ///   - 4 bytes: argument length (u32, little-endian)
    ///   - N bytes: argument string (UTF-8)
    pub fn serialize<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Write command (Option<String>)
        if let Some(ref cmd) = self.command {
            writer
                .write_all(&[1u8])
                .context("failed to write has_command flag")?;
            let cmd_bytes = cmd.as_bytes();
            let len_bytes = (cmd_bytes.len() as u32).to_le_bytes();
            writer
                .write_all(&len_bytes)
                .context("failed to write command length")?;
            writer
                .write_all(cmd_bytes)
                .context("failed to write command")?;
        } else {
            writer
                .write_all(&[0u8])
                .context("failed to write has_command flag")?;
        }

        // Write argument count
        let arg_count_bytes = (self.arguments.len() as u32).to_le_bytes();
        writer
            .write_all(&arg_count_bytes)
            .context("failed to write argument count")?;

        // Write arguments
        for arg in &self.arguments {
            let arg_bytes = arg.as_bytes();
            let len_bytes = (arg_bytes.len() as u32).to_le_bytes();
            writer
                .write_all(&len_bytes)
                .context("failed to write argument length")?;
            writer
                .write_all(arg_bytes)
                .context("failed to write argument")?;
        }

        writer.flush().context("failed to flush writer")?;
        Ok(())
    }

    /// Deserialize a request from a byte stream
    pub fn deserialize<R: Read>(reader: &mut R) -> Result<Self> {
        // Read has_command flag
        let mut has_command = [0u8; 1];
        reader
            .read_exact(&mut has_command)
            .context("failed to read has_command flag")?;

        // Read command if present
        let command = if has_command[0] == 1 {
            let mut len_bytes = [0u8; 4];
            reader
                .read_exact(&mut len_bytes)
                .context("failed to read command length")?;
            let len = u32::from_le_bytes(len_bytes) as usize;

            let mut cmd_bytes = vec![0u8; len];
            reader
                .read_exact(&mut cmd_bytes)
                .context("failed to read command")?;

            Some(String::from_utf8(cmd_bytes).context("invalid UTF-8 in command")?)
        } else if has_command[0] == 0 {
            None
        } else {
            bail!("invalid has_command flag: {}", has_command[0]);
        };

        // Read argument count
        let mut arg_count_bytes = [0u8; 4];
        reader
            .read_exact(&mut arg_count_bytes)
            .context("failed to read argument count")?;
        let arg_count = u32::from_le_bytes(arg_count_bytes) as usize;

        // Read arguments
        let mut arguments = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            let mut len_bytes = [0u8; 4];
            reader
                .read_exact(&mut len_bytes)
                .context("failed to read argument length")?;
            let len = u32::from_le_bytes(len_bytes) as usize;

            let mut arg_bytes = vec![0u8; len];
            reader
                .read_exact(&mut arg_bytes)
                .context("failed to read argument")?;

            arguments.push(String::from_utf8(arg_bytes).context("invalid UTF-8 in argument")?);
        }

        Ok(ExecRequest { command, arguments })
    }
}

impl ExecResponse {
    /// Serialize the response to a byte stream
    ///
    /// Format:
    /// - 1 byte: response type (0 = Ok, 1 = Error)
    /// - if Error:
    ///   - 4 bytes: error message length (u32, little-endian)
    ///   - N bytes: error message string (UTF-8)
    pub fn serialize<W: Write>(&self, writer: &mut W) -> Result<()> {
        match self {
            ExecResponse::Ok => {
                writer
                    .write_all(&[0u8])
                    .context("failed to write response type")?;
            }
            ExecResponse::Error(msg) => {
                writer
                    .write_all(&[1u8])
                    .context("failed to write response type")?;
                let msg_bytes = msg.as_bytes();
                let len_bytes = (msg_bytes.len() as u32).to_le_bytes();
                writer
                    .write_all(&len_bytes)
                    .context("failed to write error message length")?;
                writer
                    .write_all(msg_bytes)
                    .context("failed to write error message")?;
            }
        }

        writer.flush().context("failed to flush writer")?;
        Ok(())
    }

    /// Deserialize a response from a byte stream
    pub fn deserialize<R: Read>(reader: &mut R) -> Result<Self> {
        // Read response type
        let mut response_type = [0u8; 1];
        reader
            .read_exact(&mut response_type)
            .context("failed to read response type")?;

        match response_type[0] {
            0 => Ok(ExecResponse::Ok),
            1 => {
                // Read error message
                let mut len_bytes = [0u8; 4];
                reader
                    .read_exact(&mut len_bytes)
                    .context("failed to read error message length")?;
                let len = u32::from_le_bytes(len_bytes) as usize;

                let mut msg_bytes = vec![0u8; len];
                reader
                    .read_exact(&mut msg_bytes)
                    .context("failed to read error message")?;

                let msg = String::from_utf8(msg_bytes).context("invalid UTF-8 in error message")?;
                Ok(ExecResponse::Error(msg))
            }
            t => bail!("invalid response type: {}", t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_exec_request_serialize_deserialize() {
        // Test with command
        let req = ExecRequest::new(
            Some(String::from("bash")),
            vec![String::from("-c"), String::from("echo hello")],
        );

        let mut buffer = Vec::new();
        req.serialize(&mut buffer).unwrap();

        let mut cursor = Cursor::new(buffer);
        let deserialized = ExecRequest::deserialize(&mut cursor).unwrap();

        assert_eq!(req.command, deserialized.command);
        assert_eq!(req.arguments, deserialized.arguments);

        // Test without command (default shell)
        let req2 = ExecRequest::new(None, vec![String::from("-l")]);

        let mut buffer2 = Vec::new();
        req2.serialize(&mut buffer2).unwrap();

        let mut cursor2 = Cursor::new(buffer2);
        let deserialized2 = ExecRequest::deserialize(&mut cursor2).unwrap();

        assert_eq!(req2.command, deserialized2.command);
        assert_eq!(req2.arguments, deserialized2.arguments);
    }

    #[test]
    fn test_exec_response_serialize_deserialize() {
        // Test Ok response
        let resp_ok = ExecResponse::Ok;
        let mut buffer = Vec::new();
        resp_ok.serialize(&mut buffer).unwrap();

        let mut cursor = Cursor::new(buffer);
        let deserialized = ExecResponse::deserialize(&mut cursor).unwrap();
        assert_eq!(resp_ok, deserialized);

        // Test Error response
        let resp_err = ExecResponse::Error(String::from("test error message"));
        let mut buffer2 = Vec::new();
        resp_err.serialize(&mut buffer2).unwrap();

        let mut cursor2 = Cursor::new(buffer2);
        let deserialized2 = ExecResponse::deserialize(&mut cursor2).unwrap();
        assert_eq!(resp_err, deserialized2);
    }
}
