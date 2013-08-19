use std::bool;
use std::iterator;
use std::str;
use std::vec;

static UNEXPECTED_EOS: &'static str = "Unexpected end of stream.";

/// All the instructions that the virtual machine understands
#[deriving(Clone)]
pub enum Instruction {
    /// match one character
    Char(char),
    /// unconditional jump
    Jmp(uint),
    /// successful match
    Match,
    /// split current virtual thread into two
    Split(uint, uint),
}

enum IterResult {
    Matched,
    Continue,
    Halt,
}

/// Compiled version of a regular expression,
/// to be executed by a virtual machine
pub type CompiledRegexp = ~[Instruction];

pub struct Vm {
    program: ~[Instruction],
    ips: ~[uint],
}

type Iter<'self> = iterator::Peekable<(uint, char), str::CharOffsetIterator<'self>>;

pub struct Compiler<'self> {
    iter: Iter<'self>,
}

impl<'self> Compiler<'self> {
    pub fn new<'a>(pattern: &'a str) -> Compiler<'a> {
        Compiler {
            iter: pattern.char_offset_iter().peekable(),
        }
    }


    pub fn compile(&mut self) -> Result<CompiledRegexp, ~str> {
        match self.compile_fragment(None) {
            Ok((p, _)) => {
                let mut pm = p;
                pm.push(Match);
                Ok(pm)
            },
            Err(e) => Err(e),
        }
    }

    fn compile_fragment(&mut self, delimiter: Option<char>)
        -> Result<(CompiledRegexp, bool), ~str> {
        let mut program = ~[];
        let mut fragment = ~[];
        let mut found_delimiter = false;
        loop {
            match self.compile_one() {
                Ok(p) => program = Compiler::link(program, p),
                Err(e) => return Err(e),
            };
            match self.iter.peek() {
                Some(&(_, c)) => if c == '|' && fragment.is_empty() {
                    self.iter.next();
                    fragment = program;
                    program = ~[];
                } else if c == '|' {
                    self.iter.next();
                    fragment = Compiler::link_or(fragment, program);
                    program = ~[];
                } else if delimiter.map_default(false, |&dc| dc == c) {
                    self.iter.next();
                    found_delimiter = true;
                    break;
                },
                None => break,
            };
        }

        if fragment.is_empty() {
            Ok((program, found_delimiter))
        } else {
            Ok((Compiler::link_or(fragment, program), found_delimiter))
        }
    }

    fn link(p1: CompiledRegexp, p2: CompiledRegexp) -> CompiledRegexp {
        let len = p1.len();
        let mut pm = p2;
        for i in range(0, pm.len()) {
            match pm[i] {
                Split(a, b) => pm[i] = Split(len+a, len+b),
                Jmp(a) => pm[i] = Jmp(len+a),
                _ => {},
            }
        }
        vec::append(p1, pm)
    }

    fn link_or(p1: CompiledRegexp, p2: CompiledRegexp) -> CompiledRegexp {
        let len1 = p1.len();
        let len2 = p2.len();
        let mut pm = p1;
        pm = Compiler::link(~[Split(1, len1+2)], pm);
        pm.push(Jmp(len1+len2+2));
        Compiler::link(pm, p2)
    }

    fn compile_one(&mut self) -> Result<CompiledRegexp, ~str> {
        let mut program = ~[];
        match self.iter.next() {
            Some((i, c)) => match c {
                '?' | '*' | '+' | ')' | '|' =>
                    return Err(fmt!("Unexpected char '%c' at %u.", c, i)),
                '(' => match self.compile_group() {
                    Ok(p) => program = p,
                    Err(e) => return Err(e),
                },
                _ => program.push(Char(c)),
            },
            None => return Ok(program),
        };
        let len = program.len();
        match self.iter.peek() {
            Some(&(_, ch)) => {
                match ch {
                    '?' => {
                        program = Compiler::link(~[Split(1, len+1)], program);
                        self.iter.next();
                    },
                    '*' => {
                        program = Compiler::link(~[Split(1, len+2)], program);
                        program.push(Jmp(0));
                        self.iter.next();
                    },
                    '+' => {
                        program.push(Split(0, len+1));
                        self.iter.next();
                    },
                    _ => {},
                }
            },
            None => {},
        };
        Ok(program)
    }

    fn compile_group(&mut self) -> Result<CompiledRegexp, ~str> {
        match self.compile_fragment(Some(')')) {
            Ok((p, found_delimiter)) => if found_delimiter {
                Ok(p)
            } else {
                Err(UNEXPECTED_EOS.to_owned())
            },
            Err(e) => Err(e),
        }
    }
}

pub fn compile(pattern: &str) -> Result<Vm, ~str> {
    let mut compiler = Compiler::new(pattern);
    match compiler.compile() {
        Ok(p) => Ok(Vm::new(p)),
        Err(e) => Err(e),
    }
}

impl Vm {
    pub fn new(program: ~[Instruction]) -> Vm {
        Vm {
            program: program,
            ips: ~[],
        }
    }

    pub fn matches(&mut self, string: &str) -> bool {
        let mut iter = string.char_offset_iter();
        for _ in range(0, string.char_len()) {
            self.init();
            for (_, c) in iter.clone() {
                match self.iterate(c) {
                    Matched => return true,
                    Halt => break,
                    _ => {},
                }
            }
            for addr in self.ips.iter() {
                match self.program[*addr] {
                    Match => return true,
                    _ => {},
                }
            }
            iter.next();
        }
        false
    }

    fn init(&mut self) {
        self.ips = self.follow_jump(0);
        if self.ips.is_empty() {
            self.ips.push(0);
        }
    }

    fn iterate(&mut self, c: char) -> IterResult {
        if self.ips.is_empty() {
            return Halt;
        } else {
            let mut new_ips = ~[];
            let mut result = Continue;
            for addr in self.ips.iter() {
                match self.program[*addr] {
                    Char(ch) => if ch == c {
                        let new_addrs = self.follow_jump(*addr+1);
                        if new_addrs.is_empty() {
                            new_ips.push(*addr+1);
                        } else {
                            new_ips = vec::append(new_ips, new_addrs);
                        }
                    },
                    Match => result = Matched,
                    _ => fail!("Unexpected jump instruction."),
                }
            }
            self.ips = new_ips;
            result
        }
    }

    fn follow_jump(&self, i: uint) -> ~[uint] {
        let mut addresses = ~[];
        let mut working_set = ~[i];
        while bool::not(working_set.is_empty()) {
            let mut new_working_set = ~[];
            for address in working_set.iter() {
                match self.program[*address] {
                    Split(a, b) => {
                        new_working_set.push(a);
                        new_working_set.push(b);
                    },
                    Jmp(a) => new_working_set.push(a),
                    _ => addresses.push(*address),
                }
            }
            working_set = new_working_set;
        }
        addresses
    }
}

fn main() {
    // let s = ~"a?b+c*|d*|e+";
    // let s = ~"a+b+|a+b+";
    // let s = ~"c(a+(bd)+)+";
    let s = ~"baa*";
    match compile(s) {
        Ok(p) => {
            let mut pm = p;
            printfln!(pm);
            printfln!(pm.matches("baa"));
        },
        Err(e) => println(e),
    }
}
