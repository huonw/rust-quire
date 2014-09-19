use std::default::Default;
use std::collections::TreeMap;

use super::tokenizer::Pos;
use P = super::parser;
use T = super::tokenizer;

enum Warning {
    InvalidTag(Pos),
    NonScalarKey(Pos),
    UnsupportedTag(Pos),
    WrongNodeToMerge(Pos),
}

struct Options {
    merges: bool,
    aliases: bool,
}

impl Default for Options {
    fn default() -> Options {
        return Options {
            merges: true,
            aliases: true,
        }
    }
}

enum ScalarKind {
    Plain,
    Quoted,
}

enum NullKind {
    Implicit,
    Explicit,
}

enum Tag {
    NonSpecific,
    LocalTag(String),
    GlobalTag(String),
}

enum Ast {
    Map(Pos, Tag, TreeMap<String, Ast>),
    List(Pos, Tag, Vec<Ast>),
    Scalar(Pos, Tag, ScalarKind, String),
    Null(Pos, Tag, NullKind),
}

struct Context<'a> {
    options: &'a Options,
    document: &'a P::Document<'a>,
    warnings: Vec<Warning>,
}

fn pos_for_node<'x>(node: &P::Node<'x>) -> Pos {
    match *node {
        P::Map(_, _, _, ref tokens) => tokens[0].start.clone(),
        P::List(_, _, _, ref tokens) => tokens[0].start.clone(),
        P::Scalar(_, _, _, ref token) => token.start.clone(),
        P::ImplicitNull(_, _, ref pos) => pos.clone(),
        P::Alias(_, ref token) => token.start.clone(),
    }
}

impl<'a> Context<'a> {
    fn process(&mut self, node: &P::Node<'a>) -> Ast {
        match *node {
            P::Map(ref origtag, _, _, ref tokens) => {
                let pos = tokens[0].start.clone();
                let tag = self.string_to_tag(&pos, origtag);
                let mut mapping = TreeMap::new();
                self.merge_mapping(&mut mapping, node);

                return Map(pos, tag, mapping);
            }
            P::List(ref origtag, _, _, ref tokens) => {
                let pos = tokens[0].start.clone();
                let tag = self.string_to_tag(&pos, origtag);
                let mut seq = Vec::new();
                self.merge_sequence(&mut seq, node);

                return List(pos, tag, seq);
            }
            P::Scalar(ref tag, _, ref val, ref tok) => {
                let pos = tok.start.clone();
                let tag = self.string_to_tag(&pos, tag);
                if tok.kind == T::PlainString {
                    if val.as_slice() == "~" || val.as_slice() == "null" {
                        return Null(tok.start.clone(), tag, Explicit);
                    } else {
                        return Scalar(pos, tag, Plain, val.clone());
                    }
                } else {
                    return Scalar(pos, tag, Quoted, val.clone());
                }
            }
            P::ImplicitNull(ref tag, _, ref pos) => {
                let tag = self.string_to_tag(pos, tag);
                return Null(pos.clone(), tag, Implicit);
            }
            P::Alias(_, _) => {
                unimplemented!();
            }
        }
    }

    fn merge_mapping(&mut self, target: &mut TreeMap<String, Ast>,
        node: &P::Node<'a>)
    {
        match *node {
            P::Map(_, _, ref children, _) => {
                let mut merge = None;
                for (k, v) in children.iter() {
                    let string_key = match *k {
                        P::Scalar(_, _, ref key, ref tok) => {
                            if tok.kind == T::PlainString && key.as_slice() == "<<" {
                                merge = Some(v);
                                continue;
                            }
                            key.clone()
                        }
                        P::ImplicitNull(_, _, _) => "".to_string(),
                        P::Alias(_, _) => {
                            unimplemented!();
                        }
                        ref node => {
                            self.warnings.push(
                                NonScalarKey(pos_for_node(node)));
                            continue;
                        }
                    };
                    let value = self.process(v);
                    if !target.contains_key(&string_key) {
                        target.insert(string_key, value);
                    }
                }
                match merge {
                    Some(node) => self.merge_mapping(target, node),
                    _ => {}
                }
            }
            P::List(_, _, ref lst, _) => {
                // TODO(tailhook) check and assert on tags?
                for item in lst.iter() {
                    self.merge_mapping(target, item);
                }
            }
            P::Alias(_, _) => {
                unimplemented!();
            }
            _ => {
                self.warnings.push(WrongNodeToMerge(pos_for_node(node)));
            }
        }
    }

    fn merge_sequence(&mut self, target: &mut Vec<Ast>,
        node: &P::Node<'a>)
    {
        match *node {
            P::List(_, _, ref children, _) => {
                for item in children.iter() {
                    match *item {
                        P::List(Some("!Unpack"), _, ref children, _) => {
                            for child in children.iter() {
                                self.merge_sequence(target, child);
                            }
                        }
                        _ => {
                            let value = self.process(item);
                            target.push(value);
                        }
                    }
                }
            }
            P::Alias(_, _) => {
                unimplemented!();
            }
            _ => {
                self.warnings.push(WrongNodeToMerge(pos_for_node(node)));
            }
        }
    }

    fn string_to_tag(&mut self, pos: &Pos, src: &Option<&'a str>)
        -> Tag
    {
        match *src {
            Some(val) => {
                let mut pieces = val.splitn('!', 2);
                assert!(pieces.next().unwrap() == "");
                match (pieces.next().unwrap(), pieces.next()) {
                    ("", None) => {
                        self.warnings.push(InvalidTag(pos.clone()));
                        NonSpecific
                    }
                    (val, None) => {
                        LocalTag(val.slice_from(1).to_string())
                    }
                    ("", Some(val)) => {
                        self.warnings.push(UnsupportedTag(pos.clone()));
                        NonSpecific
                    }
                    (_, Some(val)) => {
                        self.warnings.push(UnsupportedTag(pos.clone()));
                        NonSpecific
                    }
                }
            }
            None => NonSpecific,
        }
    }
}


fn process<'x>(opt: &'x Options, doc: &'x P::Document<'x>)
    -> (Ast, Vec<Warning>)
{

    let mut ctx = Context {
        options: opt,
        document: doc,
        warnings: Vec::new(),
    };
    let ast = ctx.process(&doc.root);
    return (ast, ctx.warnings);
}
