use crate::lcg::{Lcg, LcgPositional};
use crate::xoroshiro::{Xoroshiro, XoroshiroPositional};

#[derive(Clone, Debug)]
pub enum RootSource {
    Xoroshiro(Xoroshiro),
    Lcg(Lcg),
}

impl RootSource {
    pub fn next_int(&mut self) -> i32 {
        match self {
            Self::Xoroshiro(x) => x.next_int(),
            Self::Lcg(l) => l.next_int(),
        }
    }

    pub fn next_int_bounded(&mut self, n: i32) -> i32 {
        match self {
            Self::Xoroshiro(x) => x.next_int_bounded(n),
            Self::Lcg(l) => l.next_int_bounded(n),
        }
    }

    pub fn next_long(&mut self) -> i64 {
        match self {
            Self::Xoroshiro(x) => x.next_long(),
            Self::Lcg(l) => l.next_long(),
        }
    }

    pub fn next_boolean(&mut self) -> bool {
        match self {
            Self::Xoroshiro(x) => x.next_boolean(),
            Self::Lcg(l) => l.next_boolean(),
        }
    }

    pub fn next_float(&mut self) -> f32 {
        match self {
            Self::Xoroshiro(x) => x.next_float(),
            Self::Lcg(l) => l.next_float(),
        }
    }

    pub fn next_double(&mut self) -> f64 {
        match self {
            Self::Xoroshiro(x) => x.next_double(),
            Self::Lcg(l) => l.next_double(),
        }
    }

    pub fn consume(&mut self, n: u32) {
        match self {
            Self::Xoroshiro(x) => x.consume(n),
            Self::Lcg(l) => l.consume(n),
        }
    }

    pub fn fork(&mut self) -> RootSource {
        match self {
            Self::Xoroshiro(x) => RootSource::Xoroshiro(x.fork()),
            Self::Lcg(l) => RootSource::Lcg(l.fork()),
        }
    }

    pub fn fork_positional(&mut self) -> PositionalFactory {
        match self {
            Self::Xoroshiro(x) => PositionalFactory::Xoroshiro(x.fork_positional()),
            Self::Lcg(l) => PositionalFactory::Lcg(l.fork_positional()),
        }
    }
}

#[derive(Clone, Debug)]
pub enum PositionalFactory {
    Xoroshiro(XoroshiroPositional),
    Lcg(LcgPositional),
}

impl PositionalFactory {
    pub fn at(&self, x: i32, y: i32, z: i32) -> RootSource {
        match self {
            Self::Xoroshiro(p) => RootSource::Xoroshiro(p.at(x, y, z)),
            Self::Lcg(p) => RootSource::Lcg(p.at(x, y, z)),
        }
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn from_seed(&self, s: i64) -> RootSource {
        match self {
            Self::Xoroshiro(p) => RootSource::Xoroshiro(p.from_seed(s)),
            Self::Lcg(p) => RootSource::Lcg(p.from_seed(s)),
        }
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn from_hash_of(&self, name: &str) -> RootSource {
        match self {
            Self::Xoroshiro(p) => RootSource::Xoroshiro(p.from_hash_of(name)),
            Self::Lcg(p) => RootSource::Lcg(p.from_hash_of(name)),
        }
    }
}

pub fn root(seed: i64, legacy: bool) -> RootSource {
    if legacy {
        RootSource::Lcg(Lcg::from_seed(seed))
    } else {
        RootSource::Xoroshiro(Xoroshiro::from_seed(seed))
    }
}

pub fn positional_factory(seed: i64, legacy: bool) -> PositionalFactory {
    root(seed, legacy).fork_positional()
}

pub fn noise_source(p: &PositionalFactory, key: &str) -> RootSource {
    p.from_hash_of(key)
}

pub fn terrain_noise_source(p: &PositionalFactory, seed: i64, legacy: bool) -> RootSource {
    if legacy {
        RootSource::Lcg(Lcg::from_seed(seed))
    } else {
        p.from_hash_of("minecraft:terrain")
    }
}

pub fn aquifer_factory(p: &PositionalFactory) -> PositionalFactory {
    p.from_hash_of("minecraft:aquifer").fork_positional()
}

pub fn clay_bands_source(p: &PositionalFactory) -> RootSource {
    p.from_hash_of("minecraft:clay_bands")
}

pub fn nether_temperature_source(seed: i64) -> Lcg {
    Lcg::from_seed(seed)
}

pub fn nether_vegetation_source(seed: i64) -> Lcg {
    Lcg::from_seed(seed.wrapping_add(1))
}

pub fn end_outer_islands_source(seed: i64) -> Lcg {
    Lcg::from_seed(seed)
}
