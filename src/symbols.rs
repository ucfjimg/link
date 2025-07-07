use std::cmp::max;
use std::collections::HashMap;
use crate::linker_error::LinkerError;

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub struct PublicSymbol {
    pub group: usize,
    pub segment: usize,
    pub frame: u16,
    pub offset: u16,
    pub used: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub struct CommonSymbol {
    pub size: u32,
    pub isfar: bool,
    pub group: usize,
    pub segment: usize,
    pub offset: u16,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Symbol {
    Undefined,
    Public(PublicSymbol),
    Common(CommonSymbol),
}

impl Symbol {
    pub fn public(group: usize, segment: usize, frame: u16, offset: u16) -> Self {
        Self::Public(PublicSymbol { group, segment, frame, offset, used: false })
    }

    pub fn common(size: u32, isfar: bool) -> Self {
        Self::Common(CommonSymbol { size, isfar, group: 0, segment: 0, offset: 0 })
    }
}

pub struct SymbolTable {
    pub symbols: HashMap<String, Symbol>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
        }
    }

    pub fn undefined_symbols(&self) -> Vec<&String> {
        self.symbols.keys().filter(|s| self.symbols[*s] == Symbol::Undefined).collect()
    }
    
    pub fn update(&mut self, name: &str, symbol: Symbol) -> Result<(), LinkerError> {
        //
        // Check redefinition rules
        //
        let mut exists = false;
        if symbol != Symbol::Undefined {
            if let Some(sym) = self.symbols.get(name) {
                match sym {
                    Symbol::Undefined => { exists = true; },
                    Symbol::Public(_) => {
                        return if let &Symbol::Public(_) = &symbol {
                            Err(LinkerError::new(&format!("Public symbol {} is multiply defined.", name)))
                        } else {
                            Err(LinkerError::new(&format!("Public symbol {} is redefined as communal variable.", name)))
                        };
                    },
                    Symbol::Common(_) => {
                        if let &Symbol::Public(_) = &symbol {
                            return Err(LinkerError::new(&format!("Common variable {} is redefined as public symbol.", name)));
                        }
                    },
                };
            }
        } else if let Some(sym) = self.symbols.get_mut(name) {
            if let Symbol::Public(public) = sym {
                public.used = true;
            }
            
            //
            // Don't let a future EXTDEF undefine an existing symbol.
            //
            return Ok(())
        }

        if let Symbol::Common(newsym) = &symbol {
            match self.symbols.get_mut(name) {
                Some(Symbol::Common(oldsym)) => {
                    if newsym.isfar != oldsym.isfar {
                        return Err(LinkerError::new(&format!("Attempt to change near/far attribute of common variable {}", name)));
                    }

                    oldsym.size = max(oldsym.size, newsym.size);
                    return Ok(());
                },
                _ => {},
            }
        }

        self.symbols.insert(name.to_string(), symbol);

        if exists {
            if let Some(Symbol::Public(public)) = self.symbols.get_mut(name) {
                public.used = true;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::{Symbol, SymbolTable, LinkerError};

    #[test]
    fn undefined_symbols() -> Result<(), LinkerError> {
        let mut symbols = SymbolTable::new();

        // Add one undefined symbol.
        //
        let symbol = Symbol::Undefined;
        symbols.update("main", symbol)?;

        // And one defined symbol.
        //
        let symbol = Symbol::public(1, 1, 0, 0);
        symbols.update("printf", symbol)?;

        // "main" should be undefined.
        //
        let undefs = symbols.undefined_symbols();
        assert_eq!(undefs, ["main"]);

        // Define "main" and the undef list should be empty.
        //
        let symbol = Symbol::public(1, 1, 0, 0);
        symbols.update("main", symbol)?;

        let undefs = symbols.undefined_symbols();
        assert!(undefs.is_empty());

        Ok(())
    }

    #[test]
    fn undefined_to_public() -> Result<(), LinkerError> {
        let mut symbols = SymbolTable::new();

        // Add as undefined symbol.
        //
        let symbol = Symbol::Undefined;
        symbols.update("main", symbol)?;

        // Upgrade it to be defined.
        //
        let symbol = Symbol::public(1, 1, 0, 0);
        symbols.update("main", symbol)?;


        let symbol = symbols.symbols.get("main").unwrap();

        match symbol {
            &Symbol::Public(public) => {
                assert_eq!(public.group, 1);
                assert_eq!(public.segment, 1);
                assert_eq!(public.frame, 0);
                assert_eq!(public.offset, 0);
            },
            _ => panic!("symbol is not public")
        };
        
        Ok(())
    }

    #[test]
    fn downgrade_ignored() -> Result<(), LinkerError> {
        let mut symbols = SymbolTable::new();

        // Public symbol.
        //
        let symbol = Symbol::public(1, 1, 0, 0);
        symbols.update("main", symbol)?;

        // This should succeed
        //
        let symbol = Symbol::Undefined;
        symbols.update("main", symbol)?;

        // But the symbol should remain public
        //


    
        Ok(())
    }

    #[test]
    fn bad_upgrade() -> Result<(), LinkerError> {
        let mut symbols = SymbolTable::new();

        // Public symbol.
        //
        let symbol = Symbol::public(1, 1, 0, 0);
        symbols.update("main", symbol)?;

        // Cannot change to another kind of symbol.
        //
        let symbol = Symbol::common(1234, false);
        assert!(symbols.update("main", symbol).is_err());

        Ok(())
    }

    #[test]
    fn multiply_defined() -> Result<(), LinkerError> {
        let mut symbols = SymbolTable::new();

        // Public symbol.
        //
        let symbol = Symbol::public(1, 1, 0, 0);
        symbols.update("main", symbol)?;

        // Cannot set to public once already public
        //
        let symbol = Symbol::public(1, 1, 0, 0);
        assert!(symbols.update("main", symbol).is_err());

        Ok(())
    }

    #[test]
    fn common_grows() -> Result<(), LinkerError> {
        let mut symbols = SymbolTable::new();

        // Public symbol.
        //
        let symbol = Symbol::common(100, false);
        symbols.update("buffer", symbol)?;


        let symbol = Symbol::common(200, false);
        symbols.update("buffer", symbol)?;

        match symbols.symbols.get("buffer") {
            Some(Symbol::Common(common)) => {
                assert_eq!(common.size, 200);
            },
            _ => panic!("invalid symbol type")
        }

        Ok(())
    }


    #[test]
    fn common_() -> Result<(), LinkerError> {
        let mut symbols = SymbolTable::new();

        // Public symbol.
        //
        let symbol = Symbol::common(100, false);
        symbols.update("buffer", symbol)?;


        // Try to change near to far.
        //
        let symbol = Symbol::common(200, true);
        assert!(symbols.update("buffer", symbol).is_err());

        Ok(())
    }

}