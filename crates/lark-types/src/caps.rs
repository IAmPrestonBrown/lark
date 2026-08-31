//! What the selected collector supports.
//!
//! Rule R-1 makes the transpiler read the capabilities of the collector at
//! build time and enforce the source rules that depend on them. The language
//! does not change with the collector. The set of programs that the collector
//! accepts does.
//!
//! The table here mirrors `lark_gc_caps` in the runtime. A collector states
//! the same answers twice, once for the transpiler and once for the program.
//! The test `collector_capabilities_match_the_runtime` compares them.

/// What one collector supports. See chapter 10 section 4.
///
/// The struct is a table of independent flags, and it mirrors `lark_gc_caps`
/// in the runtime one for one. Grouping them into an enum would hide that
/// correspondence and make the test that compares the two impossible to write.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capabilities {
    /// Rule M-8. The collector resolves an address inside an object.
    pub interior_pointers: bool,
    /// The collector moves an object during a collection.
    pub moving: bool,
    /// A collection frees what nothing reaches.
    pub reclaims: bool,
    /// A store of a managed pointer into a managed object needs a call.
    ///
    /// Rule R-2. A collector that walks part of the heap cannot find a
    /// pointer from the part it skips into the part it walks. The barrier
    /// records the store, so the collector finds it.
    pub write_barrier: bool,
}

impl Capabilities {
    /// Returns the capabilities of a collector, by the name `gc.strategy` uses.
    ///
    /// An unknown name yields `None`. The driver reports that separately, so
    /// this returns no answer rather than a guess.
    #[must_use]
    pub fn of(strategy: &str) -> Option<Self> {
        match strategy {
            "precise-marksweep" => Some(Self {
                interior_pointers: true,
                moving: false,
                reclaims: true,
                write_barrier: false,
            }),
            "arena" => Some(Self {
                interior_pointers: true,
                moving: false,
                reclaims: false,
                write_barrier: false,
            }),
            "semispace" => Some(Self {
                interior_pointers: false,
                moving: true,
                reclaims: true,
                write_barrier: false,
            }),
            "generational" => Some(Self {
                interior_pointers: false,
                moving: true,
                reclaims: true,
                write_barrier: true,
            }),
            _ => None,
        }
    }

    /// Reports whether the collector accepts a root mechanism.
    ///
    /// Rule R-5. A moving collector must write a new address into every root,
    /// and a rule M-13 conservative scan cannot say which words are roots.
    #[must_use]
    pub fn accepts_roots(self, roots: &str) -> bool {
        match roots {
            "conservative" => !self.moving,
            _ => true,
        }
    }
}

impl Default for Capabilities {
    /// The default collector is `precise-marksweep`. See chapter 11.
    fn default() -> Self {
        Self {
            interior_pointers: true,
            moving: false,
            reclaims: true,
            write_barrier: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Capabilities;

    /// Rule R-2. Only a collector that walks part of the heap needs a barrier.
    /// covers: R-2
    #[test]
    fn only_a_generational_collector_needs_a_barrier() {
        for name in ["precise-marksweep", "arena", "semispace"] {
            let caps = Capabilities::of(name).unwrap_or_default();
            assert!(!caps.write_barrier, "{name} asks for a barrier");
        }
        let generational = Capabilities::of("generational").unwrap_or_default();
        assert!(generational.write_barrier);
        assert!(generational.moving);
    }

    /// covers: R-1, R-4
    #[test]
    fn each_collector_reports_its_own_capabilities() {
        let sweep = Capabilities::of("precise-marksweep").unwrap_or_default();
        assert!(sweep.interior_pointers);
        assert!(!sweep.moving);
        assert!(sweep.reclaims);

        let arena = Capabilities::of("arena").unwrap_or_default();
        assert!(arena.interior_pointers);
        assert!(!arena.reclaims);

        let semispace = Capabilities::of("semispace").unwrap_or_default();
        assert!(semispace.moving);
        assert!(!semispace.interior_pointers);
    }

    /// Rule R-4. A moving collector cannot also follow an interior pointer.
    /// covers: R-4
    #[test]
    fn a_moving_collector_never_claims_interior_pointers() {
        for name in ["precise-marksweep", "arena", "semispace", "generational"] {
            let caps = Capabilities::of(name).unwrap_or_default();
            assert!(
                !(caps.moving && caps.interior_pointers),
                "{name} claims both"
            );
        }
    }

    /// covers: R-5
    #[test]
    fn a_moving_collector_refuses_a_conservative_scan() {
        let semispace = Capabilities::of("semispace").unwrap_or_default();
        assert!(semispace.accepts_roots("shadow-stack"));
        assert!(!semispace.accepts_roots("conservative"));

        let sweep = Capabilities::of("precise-marksweep").unwrap_or_default();
        assert!(sweep.accepts_roots("conservative"));
    }

    #[test]
    fn an_unknown_name_yields_no_answer() {
        assert!(Capabilities::of("nonsense").is_none());
    }
}
