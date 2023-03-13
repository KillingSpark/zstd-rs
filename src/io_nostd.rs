use alloc::boxed::Box;

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ErrorKind {
    Interrupted,
    UnexpectedEof,
    WouldBlock,
    Other,
}

impl ErrorKind {
    fn as_str(&self) -> &'static str {
        use ErrorKind::*;
        match *self {
            Interrupted => "operation interrupted",
            UnexpectedEof => "unexpected end of file",
            WouldBlock => "operation would block",
            Other => "other error",
        }
    }
}

impl core::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    err: Option<Box<dyn core::error::Error + Send + Sync>>,
}

impl Error {
    pub fn new<E>(kind: ErrorKind, err: E) -> Self
    where
        E: Into<Box<dyn core::error::Error + Send + Sync>>,
    {
        Self {
            kind,
            err: Some(err.into()),
        }
    }

    pub fn from(kind: ErrorKind) -> Self {
        Self { kind, err: None }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn get_ref(&self) -> Option<&(dyn core::error::Error + Send + Sync + 'static)> {
        self.err.as_ref().map(|e| e.as_ref())
    }

    pub fn get_mut(&mut self) -> Option<&mut (dyn core::error::Error + Send + Sync + 'static)> {
        self.err.as_mut().map(|e| e.as_mut())
    }

    pub fn into_inner(self) -> Option<Box<dyn core::error::Error + Send + Sync>> {
        self.err
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.kind.as_str())?;

        if let Some(ref e) = self.err {
            e.fmt(f)?;
        }

        Ok(())
    }
}

impl core::error::Error for Error {}

impl From<ErrorKind> for Error {
    fn from(value: ErrorKind) -> Self {
        Self::from(value)
    }
}

pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error>;

    fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<(), Error> {
        while !buf.is_empty() {
            match self.read(buf) {
                Ok(0) => break,
                Ok(n) => {
                    let tmp = buf;
                    buf = &mut tmp[n..];
                }
                Err(ref e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        if !buf.is_empty() {
            Err(Error::from(ErrorKind::UnexpectedEof))
        } else {
            Ok(())
        }
    }
}

impl Read for &[u8] {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let size = core::cmp::min(self.len(), buf.len());
        let (to_copy, rest) = self.split_at(size);

        if size == 1 {
            buf[0] = to_copy[0];
        } else {
            buf[..size].copy_from_slice(to_copy);
        }

        *self = rest;
        Ok(size)
    }
}

impl<'a, T> Read for &'a mut T
where
    T: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        (*self).read(buf)
    }
}

pub trait Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error>;
    fn flush(&mut self) -> Result<(), Error>;
}

impl<'a, T> Write for &'a mut T
where
    T: Write,
{
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        (*self).write(buf)
    }

    fn flush(&mut self) -> Result<(), Error> {
        (*self).flush()
    }
}
