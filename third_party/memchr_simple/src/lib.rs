// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

/// Naive implementations of memchr-style searches using safe loops.
/// These are used in place of the upstream memchr crate to avoid
/// SSE instructions that can trigger SIGILL under UEFI.

pub fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    for (i, &b) in haystack.iter().enumerate() {
        if b == needle {
            return Some(i);
        }
    }
    None
}

pub fn memrchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    for (i, &b) in haystack.iter().enumerate().rev() {
        if b == needle {
            return Some(i);
        }
    }
    None
}

pub fn memchr2(n1: u8, n2: u8, haystack: &[u8]) -> Option<usize> {
    for (i, &b) in haystack.iter().enumerate() {
        if b == n1 || b == n2 {
            return Some(i);
        }
    }
    None
}

pub fn memchr3(n1: u8, n2: u8, n3: u8, haystack: &[u8]) -> Option<usize> {
    for (i, &b) in haystack.iter().enumerate() {
        if b == n1 || b == n2 || b == n3 {
            return Some(i);
        }
    }
    None
}

pub fn memrchr2(n1: u8, n2: u8, haystack: &[u8]) -> Option<usize> {
    for (i, &b) in haystack.iter().enumerate().rev() {
        if b == n1 || b == n2 {
            return Some(i);
        }
    }
    None
}

pub fn memrchr3(n1: u8, n2: u8, n3: u8, haystack: &[u8]) -> Option<usize> {
    for (i, &b) in haystack.iter().enumerate().rev() {
        if b == n1 || b == n2 || b == n3 {
            return Some(i);
        }
    }
    None
}

pub struct Memchr<'h> {
    needle: u8,
    haystack: &'h [u8],
    pos: usize,
}

impl<'h> Memchr<'h> {
    pub fn new(needle: u8, haystack: &'h [u8]) -> Self {
        Self { needle, haystack, pos: 0 }
    }
}

impl<'h> Iterator for Memchr<'h> {
    type Item = usize;
    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.haystack.len() {
            let i = self.pos;
            let b = self.haystack[i];
            self.pos += 1;
            if b == self.needle {
                return Some(i);
            }
        }
        None
    }
}

pub fn memchr_iter<'h>(needle: u8, haystack: &'h [u8]) -> Memchr<'h> {
    Memchr::new(needle, haystack)
}

pub struct Memchr2<'h> {
    n1: u8,
    n2: u8,
    haystack: &'h [u8],
    pos: usize,
}

impl<'h> Memchr2<'h> {
    pub fn new(n1: u8, n2: u8, haystack: &'h [u8]) -> Self {
        Self { n1, n2, haystack, pos: 0 }
    }
}

impl<'h> Iterator for Memchr2<'h> {
    type Item = usize;
    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.haystack.len() {
            let i = self.pos;
            let b = self.haystack[i];
            self.pos += 1;
            if b == self.n1 || b == self.n2 {
                return Some(i);
            }
        }
        None
    }
}

pub fn memchr2_iter<'h>(n1: u8, n2: u8, haystack: &'h [u8]) -> Memchr2<'h> {
    Memchr2::new(n1, n2, haystack)
}

pub struct Memchr3<'h> {
    n1: u8,
    n2: u8,
    n3: u8,
    haystack: &'h [u8],
    pos: usize,
}

impl<'h> Memchr3<'h> {
    pub fn new(n1: u8, n2: u8, n3: u8, haystack: &'h [u8]) -> Self {
        Self { n1, n2, n3, haystack, pos: 0 }
    }
}

impl<'h> Iterator for Memchr3<'h> {
    type Item = usize;
    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.haystack.len() {
            let i = self.pos;
            let b = self.haystack[i];
            self.pos += 1;
            if b == self.n1 || b == self.n2 || b == self.n3 {
                return Some(i);
            }
        }
        None
    }
}

pub fn memchr3_iter<'h>(n1: u8, n2: u8, n3: u8, haystack: &'h [u8]) -> Memchr3<'h> {
    Memchr3::new(n1, n2, n3, haystack)
}

pub mod memmem {
    #[cfg(not(feature = "std"))]
    extern crate alloc;
    #[cfg(not(feature = "std"))]
    use alloc::borrow::Cow;
    #[cfg(feature = "std")]
    use std::borrow::Cow;

    /// Naive forward substring search.
    pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    /// Naive reverse substring search.
    pub fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(haystack.len());
        }
        haystack
            .windows(needle.len())
            .enumerate()
            .rev()
            .find_map(|(i, w)| if w == needle { Some(i) } else { None })
    }

    /// Iterator over forward substring matches.
    pub struct FindIter<'h, 'n> {
        haystack: &'h [u8],
        needle: &'n [u8],
        pos: usize,
    }

    impl<'h, 'n> FindIter<'h, 'n> {
        fn new(haystack: &'h [u8], needle: &'n [u8]) -> Self {
            Self { haystack, needle, pos: 0 }
        }
    }

    impl<'h, 'n> Iterator for FindIter<'h, 'n> {
        type Item = usize;
        fn next(&mut self) -> Option<Self::Item> {
            while self.pos + self.needle.len() <= self.haystack.len() {
                let i = self.pos;
                if &self.haystack[i..i + self.needle.len()] == self.needle {
                    self.pos = i + self.needle.len();
                    return Some(i);
                }
                self.pos += 1;
            }
            None
        }
    }

    /// Iterator over reverse substring matches.
    pub struct RFindIter<'h, 'n> {
        haystack: &'h [u8],
        needle: &'n [u8],
        pos: usize,
    }

    impl<'h, 'n> RFindIter<'h, 'n> {
        fn new(haystack: &'h [u8], needle: &'n [u8]) -> Self {
            Self { haystack, needle, pos: haystack.len() }
        }
    }

    impl<'h, 'n> Iterator for RFindIter<'h, 'n> {
        type Item = usize;
        fn next(&mut self) -> Option<Self::Item> {
            while self.pos >= self.needle.len() {
                let i = self.pos - self.needle.len();
                if &self.haystack[i..self.pos] == self.needle {
                    self.pos = i;
                    return Some(i);
                }
                if self.pos == 0 { break; }
                self.pos -= 1;
            }
            None
        }
    }

    #[derive(Clone, Debug)]
    pub struct Finder<'n> {
        needle: Cow<'n, [u8]>,
    }

    impl<'n> Finder<'n> {
        pub fn new<B: ?Sized + AsRef<[u8]>>(needle: &'n B) -> Finder<'n> {
            Finder { needle: Cow::Borrowed(needle.as_ref()) }
        }

        pub fn find(&self, haystack: &[u8]) -> Option<usize> {
            find(haystack, self.needle.as_ref())
        }

        pub fn find_iter<'h>(&'n self, haystack: &'h [u8]) -> FindIter<'h, 'n> {
            FindIter::new(haystack, self.needle.as_ref())
        }

        pub fn rfind(&self, haystack: &[u8]) -> Option<usize> {
            rfind(haystack, self.needle.as_ref())
        }

        pub fn needle(&self) -> &[u8] {
            self.needle.as_ref()
        }

        pub fn into_owned(self) -> Finder<'static> {
            Finder { needle: Cow::Owned(self.needle.into_owned()) }
        }
    }

    pub fn find_iter<'h, 'n, N: 'n + ?Sized + AsRef<[u8]>>(
        haystack: &'h [u8],
        needle: &'n N,
    ) -> FindIter<'h, 'n> {
        FindIter::new(haystack, needle.as_ref())
    }

    pub fn rfind_iter<'h, 'n, N: 'n + ?Sized + AsRef<[u8]>>(
        haystack: &'h [u8],
        needle: &'n N,
    ) -> RFindIter<'h, 'n> {
        RFindIter::new(haystack, needle.as_ref())
    }
}

