use std::fmt;
use std::ops::AddAssign;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Attempts(pub u8);

impl Attempts {
    pub fn new(value: u8) -> Self {
        Self(value)
    }
    
    pub fn value(&self) -> u8 {
        self.0
    }
}

impl From<u8> for Attempts {
    fn from(value: u8) -> Self {
        Self::new(value)
    }
}

impl AddAssign<u8> for Attempts {
    fn add_assign(&mut self, other: u8) {
        self.0 = self.0.wrapping_add(other);
    }
}

impl fmt::Display for Attempts {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<u8> for Attempts {
    fn eq(&self, other: &u8) -> bool {
        self.0 == *other
    }
}