// Author: Lukas Bower
// Purpose: Count production Rust risk constructs while excluding cfg-test-only syntax.
// Copyright 2026 Lukas Bower

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    Attribute, ExprMethodCall, ExprUnsafe, ForeignItem, ImplItem, Item, ItemForeignMod, ItemImpl,
    ItemTrait, Macro, Meta, Signature, Token, TraitItem, TypeBareFn,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RiskCounts {
    unsafe_count: usize,
    unwrap_count: usize,
    expect_count: usize,
    panic_count: usize,
}

impl RiskCounts {
    fn add(&mut self, other: Self) {
        self.unsafe_count += other.unsafe_count;
        self.unwrap_count += other.unwrap_count;
        self.expect_count += other.expect_count;
        self.panic_count += other.panic_count;
    }

    fn value(self, key: &str) -> Option<usize> {
        match key {
            "unsafe" => Some(self.unsafe_count),
            "unwrap" => Some(self.unwrap_count),
            "expect" => Some(self.expect_count),
            "panic" => Some(self.panic_count),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CfgValue {
    False,
    Unknown,
    True,
}

impl CfgValue {
    fn not(self) -> Self {
        match self {
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
            Self::True => Self::False,
        }
    }
}

fn eval_cfg_with_test_disabled(meta: &Meta) -> CfgValue {
    match meta {
        Meta::Path(path) => {
            if path.is_ident("test") {
                CfgValue::False
            } else {
                CfgValue::Unknown
            }
        }
        Meta::NameValue(_) => CfgValue::Unknown,
        Meta::List(list) => {
            let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
            let Ok(arguments) = parser.parse2(list.tokens.clone()) else {
                return CfgValue::Unknown;
            };
            if list.path.is_ident("all") {
                let mut saw_unknown = false;
                for argument in &arguments {
                    match eval_cfg_with_test_disabled(argument) {
                        CfgValue::False => return CfgValue::False,
                        CfgValue::Unknown => saw_unknown = true,
                        CfgValue::True => {}
                    }
                }
                if saw_unknown {
                    CfgValue::Unknown
                } else {
                    CfgValue::True
                }
            } else if list.path.is_ident("any") {
                let mut saw_unknown = false;
                for argument in &arguments {
                    match eval_cfg_with_test_disabled(argument) {
                        CfgValue::False => {}
                        CfgValue::Unknown => saw_unknown = true,
                        CfgValue::True => return CfgValue::True,
                    }
                }
                if saw_unknown {
                    CfgValue::Unknown
                } else {
                    CfgValue::False
                }
            } else if list.path.is_ident("not") && arguments.len() == 1 {
                eval_cfg_with_test_disabled(&arguments[0]).not()
            } else {
                CfgValue::Unknown
            }
        }
    }
}

fn attributes_require_test(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        if attribute.path().is_ident("test") {
            return true;
        }
        if !attribute.path().is_ident("cfg") {
            return false;
        }
        attribute
            .parse_args::<Meta>()
            .map(|meta| eval_cfg_with_test_disabled(&meta) == CfgValue::False)
            .unwrap_or(false)
    })
}

fn item_attributes(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

fn impl_item_attributes(item: &ImplItem) -> &[Attribute] {
    match item {
        ImplItem::Const(item) => &item.attrs,
        ImplItem::Fn(item) => &item.attrs,
        ImplItem::Type(item) => &item.attrs,
        ImplItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

fn trait_item_attributes(item: &TraitItem) -> &[Attribute] {
    match item {
        TraitItem::Const(item) => &item.attrs,
        TraitItem::Fn(item) => &item.attrs,
        TraitItem::Type(item) => &item.attrs,
        TraitItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

fn foreign_item_attributes(item: &ForeignItem) -> &[Attribute] {
    match item {
        ForeignItem::Fn(item) => &item.attrs,
        ForeignItem::Static(item) => &item.attrs,
        ForeignItem::Type(item) => &item.attrs,
        ForeignItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}

#[derive(Default)]
struct RiskVisitor {
    counts: RiskCounts,
}

impl<'ast> Visit<'ast> for RiskVisitor {
    fn visit_item(&mut self, item: &'ast Item) {
        if attributes_require_test(item_attributes(item)) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if attributes_require_test(impl_item_attributes(item)) {
            return;
        }
        visit::visit_impl_item(self, item);
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        if attributes_require_test(trait_item_attributes(item)) {
            return;
        }
        visit::visit_trait_item(self, item);
    }

    fn visit_foreign_item(&mut self, item: &'ast ForeignItem) {
        if attributes_require_test(foreign_item_attributes(item)) {
            return;
        }
        visit::visit_foreign_item(self, item);
    }

    fn visit_signature(&mut self, signature: &'ast Signature) {
        if signature.unsafety.is_some() {
            self.counts.unsafe_count += 1;
        }
        visit::visit_signature(self, signature);
    }

    fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
        if item.unsafety.is_some() {
            self.counts.unsafe_count += 1;
        }
        visit::visit_item_impl(self, item);
    }

    fn visit_item_trait(&mut self, item: &'ast ItemTrait) {
        if item.unsafety.is_some() {
            self.counts.unsafe_count += 1;
        }
        visit::visit_item_trait(self, item);
    }

    fn visit_item_foreign_mod(&mut self, item: &'ast ItemForeignMod) {
        if item.unsafety.is_some() {
            self.counts.unsafe_count += 1;
        }
        visit::visit_item_foreign_mod(self, item);
    }

    fn visit_type_bare_fn(&mut self, bare_fn: &'ast TypeBareFn) {
        if bare_fn.unsafety.is_some() {
            self.counts.unsafe_count += 1;
        }
        visit::visit_type_bare_fn(self, bare_fn);
    }

    fn visit_expr_unsafe(&mut self, expression: &'ast ExprUnsafe) {
        self.counts.unsafe_count += 1;
        visit::visit_expr_unsafe(self, expression);
    }

    fn visit_expr_method_call(&mut self, expression: &'ast ExprMethodCall) {
        if expression.method == "unwrap" {
            self.counts.unwrap_count += 1;
        } else if expression.method == "expect" {
            self.counts.expect_count += 1;
        }
        visit::visit_expr_method_call(self, expression);
    }

    fn visit_macro(&mut self, item: &'ast Macro) {
        if item.path.is_ident("panic") {
            self.counts.panic_count += 1;
        }

        let tokens = item.tokens.to_string();
        self.counts.unsafe_count += tokens
            .split_whitespace()
            .filter(|token| *token == "unsafe")
            .count();
        let compact: String = tokens
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        self.counts.unwrap_count += compact.matches(".unwrap(").count();
        self.counts.expect_count += compact.matches(".expect(").count();
        self.counts.panic_count += compact.matches("panic!(").count();

        visit::visit_macro(self, item);
    }
}

fn count_source(source: &str) -> Result<RiskCounts, syn::Error> {
    let syntax = syn::parse_file(source)?;
    let mut visitor = RiskVisitor::default();
    visitor.visit_file(&syntax);
    Ok(visitor.counts)
}

fn is_test_path(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(component, Component::Normal(name) if name == "test" || name == "tests")
    }) || path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with("_test.rs"))
}

fn collect_rust_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(path)
        .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("unable to read directory entry: {error}"))?;
        let child = entry.path();
        if child.is_dir() {
            if !is_test_path(&child) {
                collect_rust_files(&child, files)?;
            }
        } else if child.extension().and_then(|extension| extension.to_str()) == Some("rs")
            && !is_test_path(&child)
        {
            files.push(child);
        }
    }
    Ok(())
}

fn count_tree(root: &Path) -> Result<RiskCounts, String> {
    let mut files = Vec::new();
    for directory in ["apps", "crates"] {
        collect_rust_files(&root.join(directory), &mut files)?;
    }
    files.sort();

    let mut counts = RiskCounts::default();
    for path in files {
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
        let file_counts = count_source(&source)
            .map_err(|error| format!("unable to parse {}: {error}", path.display()))?;
        counts.add(file_counts);
    }
    Ok(counts)
}

fn read_baseline(path: &Path) -> Result<BTreeMap<String, usize>, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    let mut in_non_test = false;
    let mut values = BTreeMap::new();
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or_default().trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_non_test = line == "[non_test]";
            continue;
        }
        if !in_non_test || line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !matches!(key, "unsafe" | "unwrap" | "expect" | "panic") {
            continue;
        }
        let value = value
            .trim()
            .parse::<usize>()
            .map_err(|error| format!("invalid baseline {key} value: {error}"))?;
        values.insert(key.to_owned(), value);
    }
    for key in ["unsafe", "unwrap", "expect", "panic"] {
        if !values.contains_key(key) {
            return Err(format!("baseline missing integer for [non_test].{key}"));
        }
    }
    Ok(values)
}

fn print_counts(counts: RiskCounts, baseline: Option<&BTreeMap<String, usize>>) {
    println!("rust-risk-ratchet counts:");
    for key in ["expect", "panic", "unsafe", "unwrap"] {
        let current = counts.value(key).unwrap_or_default();
        if let Some(baseline) = baseline {
            println!(
                "  - {key}: baseline={} current={current}",
                baseline.get(key).copied().unwrap_or_default()
            );
        } else {
            println!("  - {key}: current={current}");
        }
    }
}

fn run() -> Result<(), String> {
    let mut root = PathBuf::from(".");
    let mut baseline = Some(PathBuf::from("docs/audit/rust_risk_baseline.toml"));
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--root" => {
                root = PathBuf::from(arguments.next().ok_or("--root requires a path")?);
            }
            "--baseline" => {
                baseline = Some(PathBuf::from(
                    arguments.next().ok_or("--baseline requires a path")?,
                ));
            }
            "--counts-only" => baseline = None,
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }

    let counts = count_tree(&root)?;
    let Some(baseline_path) = baseline else {
        print_counts(counts, None);
        return Ok(());
    };
    let baseline_path = if baseline_path.is_absolute() {
        baseline_path
    } else {
        root.join(baseline_path)
    };
    let baseline_values = read_baseline(&baseline_path)?;
    print_counts(counts, Some(&baseline_values));

    let mut increases = Vec::new();
    for key in ["unsafe", "unwrap", "expect", "panic"] {
        let current = counts.value(key).unwrap_or_default();
        let baseline_value = baseline_values.get(key).copied().unwrap_or_default();
        if current > baseline_value {
            increases.push(format!(
                "non-test {key} count increased: baseline={baseline_value} current={current}"
            ));
        }
    }
    if !increases.is_empty() {
        let mut message = String::from("rust-risk-ratchet failed:\n");
        for increase in increases {
            message.push_str("  - ");
            message.push_str(&increase);
            message.push('\n');
        }
        return Err(message.trim_end().to_owned());
    }

    println!("rust-risk-ratchet passed");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{count_source, eval_cfg_with_test_disabled, CfgValue, RiskCounts};

    #[test]
    fn cfg_evaluator_distinguishes_test_only_and_shared_branches() {
        let test_only: syn::Meta =
            syn::parse_str("all(not(target_os = \"none\"), test)").expect("test-only cfg parses");
        let shared: syn::Meta =
            syn::parse_str("any(test, feature = \"kernel\")").expect("shared cfg parses");
        let production: syn::Meta = syn::parse_str("not(test)").expect("production cfg parses");

        assert_eq!(eval_cfg_with_test_disabled(&test_only), CfgValue::False);
        assert_eq!(eval_cfg_with_test_disabled(&shared), CfgValue::Unknown);
        assert_eq!(eval_cfg_with_test_disabled(&production), CfgValue::True);
    }

    #[test]
    fn source_counter_excludes_inline_test_only_items() {
        let source = r#"
            fn production(value: Option<u8>) {
                let _ = value.unwrap();
                unsafe { core::ptr::read_volatile(&0u8); }
                panic!("production marker");
            }

            #[cfg(test)]
            fn test_only(value: Option<u8>) {
                let _ = value.expect("test value");
                unsafe { core::ptr::read_volatile(&0u8); }
                panic!("test marker");
            }

            #[cfg(all(not(target_os = "none"), test))]
            unsafe fn also_test_only() {}

            #[cfg(any(test, feature = "kernel"))]
            unsafe fn shared_with_production(value: Option<u8>) {
                let _ = value.expect("shared value");
            }
        "#;

        assert_eq!(
            count_source(source).expect("source parses"),
            RiskCounts {
                unsafe_count: 2,
                unwrap_count: 1,
                expect_count: 1,
                panic_count: 1,
            }
        );
    }

    #[test]
    fn source_counter_audits_macro_bodies() {
        let source = r#"
            macro_rules! risky {
                ($value:expr) => {{
                    unsafe { core::ptr::read_volatile(&0u8); }
                    $value.expect("macro value");
                    panic!("macro marker");
                }};
            }
        "#;

        assert_eq!(
            count_source(source).expect("macro source parses"),
            RiskCounts {
                unsafe_count: 1,
                unwrap_count: 0,
                expect_count: 1,
                panic_count: 1,
            }
        );
    }
}
