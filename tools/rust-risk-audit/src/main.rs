// Author: Lukas Bower
// Purpose: Count production Rust risk constructs while excluding cfg-test-only syntax.
// Copyright 2026 Lukas Bower

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use proc_macro2::{TokenStream, TokenTree};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    Attribute, Expr, ExprMethodCall, ExprPath, ExprUnsafe, ForeignItem, ImplItem, Item,
    ItemForeignMod, ItemImpl, ItemMacro, ItemTrait, Lit, Macro, Meta, Safety, Signature, Token,
    TraitItem, TypeFnPtr, UseTree,
};

const SCANNER_VERSION: &str = "rust-risk-audit/v5";
const HISTORICAL_BASELINE_COMMIT: &str = "cf8f9ee30";
const HISTORICAL_BASELINE_FULL_COMMIT: &str = "cf8f9ee30b0431dfb79a203f38ba3c7e12c86490";
const UNQUALIFIED_INCLUDE_ERROR: &str =
    "production source inclusion must use the unshadowable ::core::include! builtin";
const AUDITED_SOURCE_DIRECTORIES: [&str; 3] = ["apps", "crates", "tools"];
const TEST_ONLY_WORKSPACE_MEMBERS: [&str; 1] = ["tests"];
const HISTORICAL_ATTESTED_DIRECTORIES: [&str; 5] = ["apps", "crates", "tools", "tests", ".cargo"];
const HISTORICAL_ATTESTED_FILES: [&str; 4] = [
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "scripts/rustc-wrapper.sh",
];
const NESTED_CONFIG_SCAN_EXCLUDED_DIRECTORIES: [&str; 4] = [".git", ".venv", "out", "target"];
const LINKED_RUNTIME_HAL_PATHS: [&str; 4] = [
    "apps/pi4-driver-runtime/src",
    "apps/root-task/src/hal",
    "apps/root-task/src/drivers/driver_task_net.rs",
    "crates/pi4-driver-abi/src",
];
const LINKED_RUNTIME_HAL_CANONICAL_FILES: [&str; 4] = [
    "apps/pi4-driver-runtime/src/lib.rs",
    "apps/root-task/src/hal/mod.rs",
    "apps/root-task/src/drivers/driver_task_net.rs",
    "crates/pi4-driver-abi/src/lib.rs",
];
const EXTERNAL_NON_RUST_TREES: [&str; 1] = ["crates/sel4-sys/upstream"];
const SOURCE_SCOPE: &str = "repository-authored Rust under apps/, crates/, and tools/; package integration tests and the tests-only workspace package excluded; exact hash-pinned OUT_DIR generators, Cargo build scripts, build-script inputs and shared helper tooling, rustc wrapper, and external non-Rust trees contracted separately";
const OUT_DIR_INCLUDE_CONTRACTS: [(&str, &str, &str, &str, &str); 8] = [
    (
        "crates/sel4-sys/src/lib.rs",
        "bindings.rs",
        "crates/sel4-sys/build.rs",
        "f4f8758ff681abf47e1f3498ee2d0318bf6ee86332befe5add7737cf1b279112",
        "3e92c6248fd14a2bedd425108e6b31a4b41798ebc311df14c7afebeedf642b20",
    ),
    (
        "crates/sel4-sys/src/lib.rs",
        "sel4_config_consts.rs",
        "crates/sel4-sys/build.rs",
        "f4f8758ff681abf47e1f3498ee2d0318bf6ee86332befe5add7737cf1b279112",
        "3e92c6248fd14a2bedd425108e6b31a4b41798ebc311df14c7afebeedf642b20",
    ),
    (
        "apps/root-task/src/lib.rs",
        "built_info.rs",
        "apps/root-task/build.rs",
        "d201d01eb239c3e393d4ce38772b440d0d35146f205ac2277adb419de0a468e9",
        "1f84446ecca892a1f879fadb5f682ac073bfb302f617380cd37b831062488521",
    ),
    (
        "apps/root-task/src/hal/pi4_wifi.rs",
        "pi4_wifi_firmware.rs",
        "apps/root-task/build.rs",
        "d201d01eb239c3e393d4ce38772b440d0d35146f205ac2277adb419de0a468e9",
        "1f84446ecca892a1f879fadb5f682ac073bfb302f617380cd37b831062488521",
    ),
    (
        "apps/root-task/src/hal/driver_task.rs",
        "pi4_driver_runtime_payload.rs",
        "apps/root-task/build.rs",
        "d201d01eb239c3e393d4ce38772b440d0d35146f205ac2277adb419de0a468e9",
        "1f84446ecca892a1f879fadb5f682ac073bfb302f617380cd37b831062488521",
    ),
    (
        "apps/root-task/src/console_network_service.rs",
        "console_network_image_identity.rs",
        "apps/root-task/build.rs",
        "d201d01eb239c3e393d4ce38772b440d0d35146f205ac2277adb419de0a468e9",
        "1f84446ecca892a1f879fadb5f682ac073bfb302f617380cd37b831062488521",
    ),
    (
        "apps/root-task/src/ninedoor_service.rs",
        "ninedoor_image_identity.rs",
        "apps/root-task/build.rs",
        "d201d01eb239c3e393d4ce38772b440d0d35146f205ac2277adb419de0a468e9",
        "1f84446ecca892a1f879fadb5f682ac073bfb302f617380cd37b831062488521",
    ),
    (
        "apps/root-task/src/hal/worker_image.rs",
        "worker_image_identity.rs",
        "apps/root-task/build.rs",
        "d201d01eb239c3e393d4ce38772b440d0d35146f205ac2277adb419de0a468e9",
        "1f84446ecca892a1f879fadb5f682ac073bfb302f617380cd37b831062488521",
    ),
];
const BUILD_SCRIPT_CONTRACTS: [(&str, &str, &str); 4] = [
    (
        "apps/pi4-driver-runtime/build.rs",
        "f424066cecefb6ec3c2a974f47bde52785451b77d48ab0dd31fefc46ac8afec9",
        "aa98e6fd87c6a6a72ccc01ff5332d068dc42cb30e6d95c0147ed27d7180a7975",
    ),
    (
        "apps/root-task/build.rs",
        "d201d01eb239c3e393d4ce38772b440d0d35146f205ac2277adb419de0a468e9",
        "1f84446ecca892a1f879fadb5f682ac073bfb302f617380cd37b831062488521",
    ),
    (
        "apps/swarmui/build.rs",
        "63a93c0d7a560f4f9b4f76bf6a535f74cf0a3e811cce37779370e45346ef5d9a",
        "63a93c0d7a560f4f9b4f76bf6a535f74cf0a3e811cce37779370e45346ef5d9a",
    ),
    (
        "crates/sel4-sys/build.rs",
        "f4f8758ff681abf47e1f3498ee2d0318bf6ee86332befe5add7737cf1b279112",
        "3e92c6248fd14a2bedd425108e6b31a4b41798ebc311df14c7afebeedf642b20",
    ),
];
const BUILD_SCRIPT_INPUT_CONTRACTS: [(&str, &str, &str); 1] = [(
    "apps/root-task/build_support.rs",
    "4bee303fb82ba4412c68c47d1674edc790ed978dab086176aed0a9eba00c9134",
    "60908d93a4131cdb388fdd5d1d857abdf00e66d1ede30be26ab40c4755fbde6b",
)];
const CURRENT_ONLY_BUILD_TOOL_CONTRACTS: [(&str, &str); 2] = [
    (
        "crates/cargo-build-directive/Cargo.toml",
        "b3e346271ee20abb30f92811adcc0c3eb63fce5aeef5edf88cb4bb06a5529fd4",
    ),
    (
        "crates/cargo-build-directive/src/lib.rs",
        "ea0670945ce32ef358861e79275560c968856896ffddc93022fdecedf5c05388",
    ),
];
const RUSTC_WRAPPER_CONTRACTS: [(&str, &str, &str); 1] = [(
    "scripts/rustc-wrapper.sh",
    "7e0b859852b0bab86736fd0a14f1706fa0681dd912469cc82d18f3dff5243de4",
    "7e0b859852b0bab86736fd0a14f1706fa0681dd912469cc82d18f3dff5243de4",
)];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuditMode {
    Current,
    HistoricalReplay,
}

impl AuditMode {
    const fn allows_legacy_include(self) -> bool {
        matches!(self, Self::HistoricalReplay)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct RiskCounts {
    #[serde(rename = "unsafe")]
    unsafe_count: usize,
    #[serde(rename = "unwrap")]
    unwrap_count: usize,
    #[serde(rename = "expect")]
    expect_count: usize,
    #[serde(rename = "panic")]
    panic_count: usize,
}

const ACTIVE_GLOBAL_CEILING: RiskCounts = RiskCounts {
    unsafe_count: 828,
    unwrap_count: 38,
    expect_count: 242,
    panic_count: 102,
};
const ACTIVE_LINKED_RUNTIME_HAL_CEILING: RiskCounts = RiskCounts {
    unsafe_count: 173,
    unwrap_count: 0,
    expect_count: 2,
    panic_count: 0,
};
const ACTIVE_OUTSIDE_LINKED_RUNTIME_HAL_CEILING: RiskCounts = RiskCounts {
    unsafe_count: 655,
    unwrap_count: 38,
    expect_count: 240,
    panic_count: 102,
};
const HISTORICAL_GLOBAL_COUNTS: RiskCounts = RiskCounts {
    unsafe_count: 693,
    unwrap_count: 38,
    expect_count: 240,
    panic_count: 96,
};
const HISTORICAL_LINKED_RUNTIME_HAL_COUNTS: RiskCounts = RiskCounts {
    unsafe_count: 146,
    unwrap_count: 0,
    expect_count: 2,
    panic_count: 0,
};
const HISTORICAL_OUTSIDE_LINKED_RUNTIME_HAL_COUNTS: RiskCounts = RiskCounts {
    unsafe_count: 547,
    unwrap_count: 38,
    expect_count: 238,
    panic_count: 96,
};

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

    fn checked_sub(self, other: Self) -> Option<Self> {
        Some(Self {
            unsafe_count: self.unsafe_count.checked_sub(other.unsafe_count)?,
            unwrap_count: self.unwrap_count.checked_sub(other.unwrap_count)?,
            expect_count: self.expect_count.checked_sub(other.expect_count)?,
            panic_count: self.panic_count.checked_sub(other.panic_count)?,
        })
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

fn ident_is(ident: &syn::Ident, expected: &str) -> bool {
    let value = ident.to_string();
    value.strip_prefix("r#").unwrap_or(&value) == expected
}

fn path_last_is(path: &syn::Path, expected: &str) -> bool {
    path.segments
        .last()
        .is_some_and(|segment| ident_is(&segment.ident, expected))
}

fn path_is_absolute_core_macro(path: &syn::Path, expected: &str) -> bool {
    path.leading_colon.is_some()
        && path.segments.len() == 2
        && path
            .segments
            .first()
            .is_some_and(|segment| ident_is(&segment.ident, "core"))
        && path_last_is(path, expected)
}

fn path_targets_excluded_tests(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|component| matches!(component, Component::Normal(name) if name == "tests"))
}

#[derive(Default)]
struct SourceAudit {
    counts: RiskCounts,
    path_redirects: Vec<String>,
    include_expressions: Vec<String>,
    invalid_attributes: Vec<String>,
}

#[derive(Default)]
struct RiskVisitor {
    counts: RiskCounts,
    aliases: HashMap<String, RiskKind>,
    path_redirects: Vec<String>,
    include_expressions: Vec<String>,
    invalid_attributes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RiskKind {
    Include,
    Expect,
    Panic,
    Unwrap,
}

fn direct_risk_kind(name: &str) -> Option<RiskKind> {
    match name {
        "include" => Some(RiskKind::Include),
        "expect" => Some(RiskKind::Expect),
        "panic" => Some(RiskKind::Panic),
        "unwrap" => Some(RiskKind::Unwrap),
        _ => None,
    }
}

fn ident_risk_kind(ident: &syn::Ident, aliases: &HashMap<String, RiskKind>) -> Option<RiskKind> {
    let name = ident.to_string();
    let name = name.strip_prefix("r#").unwrap_or(&name);
    direct_risk_kind(name).or_else(|| aliases.get(name).copied())
}

fn path_risk_kind(path: &syn::Path, aliases: &HashMap<String, RiskKind>) -> Option<RiskKind> {
    path.segments
        .last()
        .and_then(|segment| ident_risk_kind(&segment.ident, aliases))
}

#[derive(Default)]
struct AliasCollector {
    aliases: HashMap<String, RiskKind>,
    renamed_risks: Vec<String>,
}

impl AliasCollector {
    fn collect_use_tree(&mut self, tree: &UseTree) {
        match tree {
            UseTree::Path(path) => self.collect_use_tree(&path.tree),
            UseTree::Name(name) => {
                let imported = name.ident.to_string();
                let imported = imported.strip_prefix("r#").unwrap_or(&imported);
                if let Some(kind) = direct_risk_kind(imported) {
                    self.aliases.insert(imported.to_owned(), kind);
                    self.renamed_risks
                        .push(format!("import of reserved risk name {imported}"));
                }
            }
            UseTree::Rename(rename) => {
                let imported = rename.ident.to_string();
                let imported = imported.strip_prefix("r#").unwrap_or(&imported);
                let alias = rename.rename.to_string();
                let alias = alias.strip_prefix("r#").unwrap_or(&alias);
                if let Some(kind) = direct_risk_kind(imported).or_else(|| direct_risk_kind(alias)) {
                    self.aliases.insert(alias.to_owned(), kind);
                    self.renamed_risks.push(format!(
                        "import of reserved risk name {imported} as {alias}"
                    ));
                }
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    self.collect_use_tree(item);
                }
            }
            UseTree::Glob(_) => {}
        }
    }
}

impl<'ast> Visit<'ast> for AliasCollector {
    fn visit_item(&mut self, item: &'ast Item) {
        if attributes_require_test(item_attributes(item)) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.collect_use_tree(&item.tree);
        visit::visit_item_use(self, item);
    }
}

impl RiskVisitor {
    fn audit_attribute_meta(&mut self, meta: &Meta) {
        if path_last_is(meta.path(), "unsafe") {
            self.counts.unsafe_count += 1;
        }
        if path_last_is(meta.path(), "path") {
            match meta {
                Meta::NameValue(name_value) => match &name_value.value {
                    Expr::Lit(expression) => match &expression.lit {
                        Lit::Str(path) => self.path_redirects.push(path.value()),
                        _ => self
                            .invalid_attributes
                            .push(String::from("path attribute must use a string literal")),
                    },
                    _ => self
                        .invalid_attributes
                        .push(String::from("path attribute must use a literal value")),
                },
                _ => self
                    .invalid_attributes
                    .push(String::from("path attribute must be name-value syntax")),
            }
        }
        if path_last_is(meta.path(), "cfg_attr") {
            let Meta::List(list) = meta else {
                self.invalid_attributes
                    .push(String::from("cfg_attr must use list syntax"));
                return;
            };
            let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
            let Ok(arguments) = parser.parse2(list.tokens.clone()) else {
                self.invalid_attributes
                    .push(String::from("cfg_attr arguments could not be parsed"));
                return;
            };
            let mut arguments = arguments.iter();
            let Some(predicate) = arguments.next() else {
                self.invalid_attributes
                    .push(String::from("cfg_attr has no predicate"));
                return;
            };
            let nested: Vec<_> = arguments.collect();
            if nested.is_empty() {
                self.invalid_attributes
                    .push(String::from("cfg_attr has no conditional attributes"));
                return;
            }
            if eval_cfg_with_test_disabled(predicate) != CfgValue::False {
                for nested_meta in nested {
                    self.audit_attribute_meta(nested_meta);
                }
            }
        }
    }
}

fn attribute_group_has_redirect(tokens: TokenStream) -> bool {
    let mut tokens = tokens.into_iter();
    match tokens.next() {
        Some(TokenTree::Ident(ident)) => ident_is(&ident, "path") || ident_is(&ident, "cfg_attr"),
        Some(TokenTree::Punct(punctuation)) => punctuation.as_char() == '$',
        _ => false,
    }
}

fn audit_macro_tokens(
    tokens: TokenStream,
    aliases: &HashMap<String, RiskKind>,
) -> (RiskCounts, bool) {
    let mut counts = RiskCounts::default();
    let mut redirect = false;
    let mut tokens = tokens.into_iter().peekable();
    while let Some(token) = tokens.next() {
        match token {
            TokenTree::Group(group) => {
                let (nested_counts, nested_redirect) = audit_macro_tokens(group.stream(), aliases);
                counts.add(nested_counts);
                redirect |= nested_redirect;
            }
            TokenTree::Punct(punctuation) if punctuation.as_char() == '#' => {
                if let Some(TokenTree::Group(group)) = tokens.peek() {
                    if group.delimiter() == proc_macro2::Delimiter::Bracket
                        && attribute_group_has_redirect(group.stream())
                    {
                        redirect = true;
                    }
                }
            }
            TokenTree::Ident(ident) if ident_is(&ident, "unsafe") => counts.unsafe_count += 1,
            TokenTree::Ident(ident) => match ident_risk_kind(&ident, aliases) {
                Some(RiskKind::Include) => redirect = true,
                Some(RiskKind::Expect) => counts.expect_count += 1,
                Some(RiskKind::Panic) => counts.panic_count += 1,
                Some(RiskKind::Unwrap) => counts.unwrap_count += 1,
                None => {}
            },
            _ => {}
        }
    }
    (counts, redirect)
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
        if matches!(signature.safety, Safety::Unsafe(_)) {
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

    fn visit_type_fn_ptr(&mut self, fn_ptr: &'ast TypeFnPtr) {
        if fn_ptr.unsafety.is_some() {
            self.counts.unsafe_count += 1;
        }
        visit::visit_type_fn_ptr(self, fn_ptr);
    }

    fn visit_expr_unsafe(&mut self, expression: &'ast ExprUnsafe) {
        self.counts.unsafe_count += 1;
        visit::visit_expr_unsafe(self, expression);
    }

    fn visit_expr_method_call(&mut self, expression: &'ast ExprMethodCall) {
        if ident_is(&expression.method, "unwrap") {
            self.counts.unwrap_count += 1;
        } else if ident_is(&expression.method, "expect") {
            self.counts.expect_count += 1;
        }
        visit::visit_expr_method_call(self, expression);
    }

    fn visit_expr_path(&mut self, expression: &'ast ExprPath) {
        match path_risk_kind(&expression.path, &self.aliases) {
            Some(RiskKind::Unwrap) => self.counts.unwrap_count += 1,
            Some(RiskKind::Expect) => self.counts.expect_count += 1,
            _ => {}
        }
        visit::visit_expr_path(self, expression);
    }

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        self.audit_attribute_meta(&attribute.meta);
        visit::visit_attribute(self, attribute);
    }

    fn visit_macro(&mut self, item: &'ast Macro) {
        match path_risk_kind(&item.path, &self.aliases) {
            Some(RiskKind::Panic) => self.counts.panic_count += 1,
            Some(RiskKind::Include) if path_is_absolute_core_macro(&item.path, "include") => {
                self.include_expressions.push(item.tokens.to_string());
            }
            Some(RiskKind::Include) => {
                self.include_expressions.push(item.tokens.to_string());
                self.invalid_attributes
                    .push(String::from(UNQUALIFIED_INCLUDE_ERROR));
            }
            _ => {}
        }
        let (macro_counts, source_redirect) =
            audit_macro_tokens(item.tokens.clone(), &self.aliases);
        if source_redirect
            && !matches!(
                path_risk_kind(&item.path, &self.aliases),
                Some(RiskKind::Include)
            )
        {
            self.invalid_attributes.push(String::from(
                "macro token tree may emit or invoke a source redirect",
            ));
        }
        self.counts.add(macro_counts);

        visit::visit_macro(self, item);
    }

    fn visit_item_macro(&mut self, item: &'ast ItemMacro) {
        if item
            .ident
            .as_ref()
            .and_then(|ident| ident_risk_kind(ident, &self.aliases))
            .is_some()
        {
            self.invalid_attributes.push(String::from(
                "macro definitions cannot shadow reserved risk names",
            ));
        }
        visit::visit_item_macro(self, item);
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        let exposed_name = item
            .rename
            .as_ref()
            .map_or(&item.ident, |(_, rename)| rename);
        if ident_is(exposed_name, "core")
            || ident_risk_kind(exposed_name, &self.aliases).is_some()
            || item
                .attrs
                .iter()
                .any(|attribute| path_last_is(attribute.path(), "macro_use"))
        {
            self.invalid_attributes.push(String::from(
                "extern crate cannot shadow core/risk names or use implicit macro imports",
            ));
        }
        visit::visit_item_extern_crate(self, item);
    }
}

fn audit_source(source: &str) -> Result<SourceAudit, syn::Error> {
    let syntax = syn::parse_file(source)?;
    let mut aliases = AliasCollector::default();
    aliases.visit_file(&syntax);
    let mut visitor = RiskVisitor {
        aliases: aliases.aliases,
        invalid_attributes: aliases
            .renamed_risks
            .into_iter()
            .map(|rename| format!("risk construct import aliases are forbidden: {rename}"))
            .collect(),
        ..RiskVisitor::default()
    };
    visitor.visit_file(&syntax);
    Ok(SourceAudit {
        counts: visitor.counts,
        path_redirects: visitor.path_redirects,
        include_expressions: visitor.include_expressions,
        invalid_attributes: visitor.invalid_attributes,
    })
}

#[cfg(test)]
fn count_source(source: &str) -> Result<RiskCounts, syn::Error> {
    let audit = audit_source(source)?;
    if let Some(error) = audit.invalid_attributes.first() {
        return Err(syn::Error::new(proc_macro2::Span::call_site(), error));
    }
    Ok(audit.counts)
}

fn is_excluded_test_path(root: &Path, path: &Path) -> bool {
    let mut current = if path.is_dir() {
        Some(path)
    } else {
        path.parent()
    };
    while let Some(directory) = current {
        if !directory.starts_with(root) {
            return false;
        }
        if directory.join("Cargo.toml").is_file() {
            return path
                .strip_prefix(directory)
                .ok()
                .and_then(|relative| relative.components().next())
                .is_some_and(
                    |component| matches!(component, Component::Normal(name) if name == "tests"),
                );
        }
        if directory == root {
            break;
        }
        current = directory.parent();
    }
    false
}

fn collect_rust_files(root: &Path, path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(path)
        .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("unable to read directory entry: {error}"))?;
        let child = entry.path();
        if is_external_non_rust_tree(root, &child) {
            continue;
        }
        let metadata = fs::symlink_metadata(&child)
            .map_err(|error| format!("unable to inspect {}: {error}", child.display()))?;
        if metadata.file_type().is_symlink() {
            let rust_or_manifest = child.extension().and_then(|value| value.to_str()) == Some("rs")
                || child.file_name().is_some_and(|name| name == "Cargo.toml");
            if rust_or_manifest || child.is_dir() {
                return Err(format!(
                    "symlink can redirect audited Rust source discovery: {}",
                    child.display()
                ));
            }
            continue;
        }
        if metadata.is_dir() {
            if !is_excluded_test_path(root, &child) {
                collect_rust_files(root, &child, files)?;
            }
        } else if metadata.is_file()
            && child.extension().and_then(|extension| extension.to_str()) == Some("rs")
            && !is_excluded_test_path(root, &child)
        {
            files.push(child);
        }
    }
    Ok(())
}

fn normalize_lexical_path(path: &Path) -> Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(format!(
                        "path escapes the filesystem root: {}",
                        path.display()
                    ));
                }
            }
            Component::Normal(value) => normalized.push(value),
        }
    }
    Ok(normalized)
}

fn is_in_audited_source_roots(root: &Path, path: &Path) -> bool {
    AUDITED_SOURCE_DIRECTORIES
        .iter()
        .any(|directory| path.starts_with(root.join(directory)))
}

fn is_external_non_rust_tree(root: &Path, path: &Path) -> bool {
    EXTERNAL_NON_RUST_TREES
        .iter()
        .any(|relative| path.starts_with(root.join(relative)))
}

fn validate_no_symlink_ancestors(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path.strip_prefix(root).map_err(|_| {
        format!(
            "audited source path escapes repository root: {}",
            path.display()
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("unable to inspect {}: {error}", current.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "audited source path traverses a symlink: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn validate_repo_source_file(
    root: &Path,
    candidate: &Path,
    context: &str,
) -> Result<PathBuf, String> {
    let normalized = normalize_lexical_path(candidate)?;
    if !is_in_audited_source_roots(root, &normalized) {
        return Err(format!(
            "{context} escapes audited apps/crates/tools source roots: {}",
            candidate.display()
        ));
    }
    if is_external_non_rust_tree(root, &normalized) {
        return Err(format!(
            "{context} redirects into the contracted external non-Rust tree: {}",
            candidate.display()
        ));
    }
    if normalized
        .extension()
        .and_then(|extension| extension.to_str())
        != Some("rs")
    {
        return Err(format!(
            "{context} must resolve to a .rs source file: {}",
            candidate.display()
        ));
    }
    if path_targets_excluded_tests(&normalized.to_string_lossy())
        || is_excluded_test_path(root, &normalized)
    {
        return Err(format!(
            "{context} redirects production source into an excluded tests tree: {}",
            candidate.display()
        ));
    }
    validate_no_symlink_ancestors(root, &normalized)?;
    let canonical = fs::canonicalize(&normalized)
        .map_err(|error| format!("unable to resolve {}: {error}", normalized.display()))?;
    if canonical != normalized {
        return Err(format!(
            "{context} does not resolve canonically without redirection: {} -> {}",
            normalized.display(),
            canonical.display()
        ));
    }
    let metadata = fs::symlink_metadata(&canonical)
        .map_err(|error| format!("unable to inspect {}: {error}", canonical.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "{context} is not a regular Rust source file: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn package_root_for_source(root: &Path, source: &Path) -> Result<PathBuf, String> {
    let mut current = source.parent();
    while let Some(directory) = current {
        if directory.join("Cargo.toml").is_file() {
            return Ok(directory.to_path_buf());
        }
        if directory == root {
            break;
        }
        current = directory.parent();
    }
    Err(format!(
        "unable to find Cargo.toml for audited source {}",
        source.display()
    ))
}

#[derive(Debug, Eq, PartialEq)]
enum IncludePath {
    Static(String),
    OutDir(String),
}

fn eval_env_macro(item: &Macro, package_root: &Path) -> Result<IncludePath, String> {
    let value: syn::LitStr = syn::parse2(item.tokens.clone())
        .map_err(|_| String::from("env! in include! must have one string-literal argument"))?;
    match value.value().as_str() {
        "CARGO_MANIFEST_DIR" => Ok(IncludePath::Static(
            package_root.to_string_lossy().into_owned(),
        )),
        "OUT_DIR" => Ok(IncludePath::OutDir(String::new())),
        variable => Err(format!(
            "include! uses unsupported dynamic environment variable {variable}"
        )),
    }
}

fn append_include_path(left: IncludePath, right: IncludePath) -> Result<IncludePath, String> {
    match (left, right) {
        (IncludePath::Static(mut left), IncludePath::Static(right)) => {
            left.push_str(&right);
            Ok(IncludePath::Static(left))
        }
        (IncludePath::OutDir(mut suffix), IncludePath::Static(right)) => {
            suffix.push_str(&right);
            Ok(IncludePath::OutDir(suffix))
        }
        (IncludePath::Static(left), IncludePath::OutDir(suffix)) if left.is_empty() => {
            Ok(IncludePath::OutDir(suffix))
        }
        _ => Err(String::from(
            "OUT_DIR may appear only once and first in an include! concat! expression",
        )),
    }
}

fn eval_include_expr(
    expression: &Expr,
    package_root: &Path,
    mode: AuditMode,
) -> Result<IncludePath, String> {
    match expression {
        Expr::Lit(expression) => match &expression.lit {
            Lit::Str(value) => Ok(IncludePath::Static(value.value())),
            _ => Err(String::from("include! path must be a string literal")),
        },
        Expr::Macro(expression)
            if path_is_absolute_core_macro(&expression.mac.path, "env")
                || (mode.allows_legacy_include()
                    && path_last_is(&expression.mac.path, "env")) =>
        {
            eval_env_macro(&expression.mac, package_root)
        }
        Expr::Macro(expression)
            if path_is_absolute_core_macro(&expression.mac.path, "concat")
                || (mode.allows_legacy_include()
                    && path_last_is(&expression.mac.path, "concat")) =>
        {
            let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
            let arguments = parser
                .parse2(expression.mac.tokens.clone())
                .map_err(|_| String::from("include! concat! arguments could not be parsed"))?;
            let mut value = IncludePath::Static(String::new());
            for argument in arguments {
                value = append_include_path(
                    value,
                    eval_include_expr(&argument, package_root, mode)?,
                )?;
            }
            Ok(value)
        }
        _ => Err(String::from(
            "::core::include! paths must use literals, ::core::concat!, ::core::env!, CARGO_MANIFEST_DIR, or a contracted OUT_DIR",
        )),
    }
}

fn validate_out_dir_include(
    root: &Path,
    source: &Path,
    suffix: &str,
) -> Result<(String, String), String> {
    let output = suffix.trim_start_matches(['/', '\\']);
    if output.is_empty()
        || output.contains(['/', '\\'])
        || Path::new(output)
            .extension()
            .and_then(|value| value.to_str())
            != Some("rs")
    {
        return Err(format!(
            "OUT_DIR include must name one contracted .rs output: {suffix}"
        ));
    }
    let relative_source = source
        .strip_prefix(root)
        .map_err(|_| format!("include source escapes repository: {}", source.display()))?
        .to_string_lossy()
        .into_owned();
    let matching: Vec<_> = OUT_DIR_INCLUDE_CONTRACTS
        .iter()
        .filter(|(contract_source, contract_output, _, _, _)| {
            *contract_source == relative_source && *contract_output == output
        })
        .collect();
    if matching.len() != 1 {
        return Err(format!(
            "OUT_DIR include lacks one exact generator contract: {relative_source} -> {output}"
        ));
    }
    let (_, _, generator, current_hash, historical_hash) = matching[0];
    let generator_path =
        validate_repo_source_file(root, &root.join(generator), "OUT_DIR include generator")?;
    let generator_bytes = fs::read(&generator_path)
        .map_err(|error| format!("unable to read {}: {error}", generator_path.display()))?;
    let actual_hash = hex::encode(Sha256::digest(&generator_bytes));
    if actual_hash != *current_hash && actual_hash != *historical_hash {
        return Err(format!(
            "OUT_DIR generator contract hash changed: {generator} expected-current={current_hash} expected-historical={historical_hash} actual={actual_hash}"
        ));
    }
    let generator_source = String::from_utf8(generator_bytes)
        .map_err(|_| format!("OUT_DIR generator is not UTF-8 Rust: {generator}"))?;
    if !generator_source.contains(output) {
        return Err(format!(
            "OUT_DIR generator contract does not reference output {output}: {generator}"
        ));
    }
    Ok((relative_source, output.to_owned()))
}

fn validate_include_expression(
    root: &Path,
    source: &Path,
    tokens: &str,
    mode: AuditMode,
) -> Result<Option<(String, String)>, String> {
    let expression: Expr = syn::parse_str(tokens)
        .map_err(|error| format!("unable to parse include! path expression {tokens}: {error}"))?;
    let package_root = package_root_for_source(root, source)?;
    match eval_include_expr(&expression, &package_root, mode)? {
        IncludePath::Static(path) => {
            let candidate = PathBuf::from(&path);
            let candidate = if candidate.is_absolute() {
                candidate
            } else {
                source
                    .parent()
                    .ok_or_else(|| format!("source has no parent: {}", source.display()))?
                    .join(candidate)
            };
            validate_repo_source_file(root, &candidate, "include! source")?;
            Ok(None)
        }
        IncludePath::OutDir(suffix) => validate_out_dir_include(root, source, &suffix).map(Some),
    }
}

fn validate_path_redirect(root: &Path, source: &Path, redirect: &str) -> Result<(), String> {
    if Path::new(redirect).is_absolute() || path_targets_excluded_tests(redirect) {
        return Err(format!(
            "production path attribute has forbidden target: {}: {redirect}",
            source.display()
        ));
    }
    let parent = source
        .parent()
        .ok_or_else(|| format!("source has no parent: {}", source.display()))?;
    validate_repo_source_file(root, &parent.join(redirect), "path attribute")?;
    Ok(())
}

fn validate_manifest_repo_path(
    root: &Path,
    package_root: &Path,
    raw_path: &str,
    context: &str,
    rust_source: bool,
) -> Result<PathBuf, String> {
    let candidate = Path::new(raw_path);
    let candidate = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        package_root.join(candidate)
    };
    let normalized = normalize_lexical_path(&candidate)?;
    if !is_in_audited_source_roots(root, &normalized) {
        return Err(format!(
            "{context} escapes audited apps/crates/tools source roots: {raw_path}"
        ));
    }
    if is_external_non_rust_tree(root, &normalized) {
        return Err(format!(
            "{context} redirects into the contracted external non-Rust tree: {raw_path}"
        ));
    }
    if path_targets_excluded_tests(raw_path) || is_excluded_test_path(root, &normalized) {
        return Err(format!(
            "{context} redirects a production Cargo target into tests: {raw_path}"
        ));
    }
    validate_no_symlink_ancestors(root, &normalized)?;
    let canonical = fs::canonicalize(&normalized)
        .map_err(|error| format!("unable to resolve {context} {raw_path}: {error}"))?;
    if canonical != normalized {
        return Err(format!(
            "{context} does not resolve canonically: {} -> {}",
            normalized.display(),
            canonical.display()
        ));
    }
    let metadata = fs::symlink_metadata(&canonical)
        .map_err(|error| format!("unable to inspect {}: {error}", canonical.display()))?;
    if rust_source {
        if canonical.extension().and_then(|value| value.to_str()) != Some("rs")
            || !metadata.is_file()
        {
            return Err(format!("{context} is not a regular .rs file: {raw_path}"));
        }
    } else if !metadata.is_dir() {
        return Err(format!("{context} is not a package directory: {raw_path}"));
    }
    Ok(canonical)
}

fn validate_dependency_table(
    root: &Path,
    package_root: &Path,
    table: Option<&toml::value::Table>,
    context: &str,
) -> Result<(), String> {
    let Some(table) = table else {
        return Ok(());
    };
    for (name, dependency) in table {
        let Some(dependency) = dependency.as_table() else {
            continue;
        };
        if dependency
            .get("git")
            .and_then(toml::Value::as_str)
            .is_some_and(is_local_file_url)
        {
            return Err(format!(
                "{context} dependency {name} uses a local file-backed git source outside deterministic source discovery"
            ));
        }
        if let Some(path) = dependency.get("path").and_then(toml::Value::as_str) {
            validate_manifest_repo_path(
                root,
                package_root,
                path,
                &format!("{context} dependency {name}"),
                false,
            )?;
        }
    }
    Ok(())
}

fn is_local_file_url(value: &str) -> bool {
    value.to_ascii_lowercase().contains("file:")
}

fn is_contracted_build_script(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .is_some_and(|relative| {
            BUILD_SCRIPT_CONTRACTS
                .iter()
                .any(|(contracted, _, _)| relative == *contracted)
        })
}

fn validate_manifest(root: &Path, manifest: &Path) -> Result<(), String> {
    let package_root = manifest
        .parent()
        .ok_or_else(|| format!("manifest has no parent: {}", manifest.display()))?;
    let mut ancestor = package_root.parent();
    while let Some(directory) = ancestor {
        if directory == root {
            break;
        }
        if directory.join("Cargo.toml").is_file() {
            return Err(format!(
                "nested Cargo manifest can hide production source behind package test exclusions: {} (ancestor package {})",
                manifest.display(),
                directory.display()
            ));
        }
        ancestor = directory.parent();
    }
    let source = fs::read_to_string(manifest)
        .map_err(|error| format!("unable to read {}: {error}", manifest.display()))?;
    let value: toml::Value = toml::from_str(&source)
        .map_err(|error| format!("unable to parse {}: {error}", manifest.display()))?;
    let table = value
        .as_table()
        .ok_or_else(|| format!("manifest is not a TOML table: {}", manifest.display()))?;

    if table
        .get("package")
        .and_then(toml::Value::as_table)
        .is_some_and(|package| package.contains_key("links"))
    {
        return Err(format!(
            "Cargo package links metadata permits target link overrides to force cfg(test): {}",
            manifest.display()
        ));
    }

    let declared_build = table
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("build"));
    match declared_build {
        Some(build) => match build {
            toml::Value::String(path) => {
                let build_script = validate_manifest_repo_path(
                    root,
                    package_root,
                    path,
                    "Cargo package build target",
                    true,
                )?;
                if !is_contracted_build_script(root, &build_script) {
                    return Err(format!(
                        "Cargo build script is not hash-contracted against cfg(test) injection: {}",
                        build_script.display()
                    ));
                }
            }
            toml::Value::Boolean(false) => {}
            _ => {
                return Err(format!(
                    "Cargo package build target must be false or a literal path: {}",
                    manifest.display()
                ));
            }
        },
        None => {
            let implicit = package_root.join("build.rs");
            if implicit.is_file() && !is_contracted_build_script(root, &implicit) {
                return Err(format!(
                    "implicit Cargo build script is not hash-contracted against cfg(test) injection: {}",
                    implicit.display()
                ));
            }
        }
    }

    if let Some(library) = table.get("lib").and_then(toml::Value::as_table) {
        let proc_macro = library
            .get("proc-macro")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false)
            || library
                .get("crate-type")
                .and_then(toml::Value::as_array)
                .is_some_and(|types| types.iter().any(|kind| kind.as_str() == Some("proc-macro")));
        if proc_macro {
            return Err(format!(
                "local proc-macro packages can emit unaudited production Rust: {}",
                manifest.display()
            ));
        }
        if let Some(path) = library.get("path").and_then(toml::Value::as_str) {
            validate_manifest_repo_path(root, package_root, path, "Cargo library target", true)?;
        }
    }
    for target_kind in ["bin", "example", "bench"] {
        if let Some(targets) = table.get(target_kind).and_then(toml::Value::as_array) {
            for target in targets {
                if let Some(path) = target
                    .as_table()
                    .and_then(|target| target.get("path"))
                    .and_then(toml::Value::as_str)
                {
                    validate_manifest_repo_path(
                        root,
                        package_root,
                        path,
                        &format!("Cargo {target_kind} target"),
                        true,
                    )?;
                }
            }
        }
    }

    validate_dependency_table(
        root,
        package_root,
        table.get("dependencies").and_then(toml::Value::as_table),
        "Cargo production",
    )?;
    validate_dependency_table(
        root,
        package_root,
        table
            .get("build-dependencies")
            .and_then(toml::Value::as_table),
        "Cargo build",
    )?;
    validate_dependency_table(
        root,
        package_root,
        table
            .get("dev-dependencies")
            .and_then(toml::Value::as_table),
        "Cargo development",
    )?;
    if let Some(targets) = table.get("target").and_then(toml::Value::as_table) {
        for (target_name, target) in targets {
            let target = target.as_table().ok_or_else(|| {
                format!(
                    "Cargo target {target_name} is not a table: {}",
                    manifest.display()
                )
            })?;
            validate_dependency_table(
                root,
                package_root,
                target.get("dependencies").and_then(toml::Value::as_table),
                &format!("Cargo target {target_name}"),
            )?;
            validate_dependency_table(
                root,
                package_root,
                target
                    .get("build-dependencies")
                    .and_then(toml::Value::as_table),
                &format!("Cargo target {target_name} build"),
            )?;
            validate_dependency_table(
                root,
                package_root,
                target
                    .get("dev-dependencies")
                    .and_then(toml::Value::as_table),
                &format!("Cargo target {target_name} development"),
            )?;
        }
    }
    Ok(())
}

fn collect_manifest_files(
    root: &Path,
    path: &Path,
    manifests: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries = fs::read_dir(path)
        .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("unable to read directory entry: {error}"))?;
        let child = entry.path();
        if is_external_non_rust_tree(root, &child) {
            continue;
        }
        let metadata = fs::symlink_metadata(&child)
            .map_err(|error| format!("unable to inspect {}: {error}", child.display()))?;
        if metadata.file_type().is_symlink() {
            let rust_or_manifest = child.extension().and_then(|value| value.to_str()) == Some("rs")
                || child.file_name().is_some_and(|name| name == "Cargo.toml");
            if rust_or_manifest || child.is_dir() {
                return Err(format!(
                    "symlink can redirect audited package discovery: {}",
                    child.display()
                ));
            }
            continue;
        }
        if metadata.is_dir() {
            collect_manifest_files(root, &child, manifests)?;
        } else if metadata.is_file() && child.file_name().is_some_and(|name| name == "Cargo.toml") {
            manifests.push(child);
        }
    }
    Ok(())
}

fn reject_nested_cargo_configs(root: &Path, path: &Path) -> Result<(), String> {
    let entries = fs::read_dir(path)
        .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("unable to read directory entry: {error}"))?;
        let child = entry.path();
        let child_name = child
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if NESTED_CONFIG_SCAN_EXCLUDED_DIRECTORIES.contains(&child_name) {
            continue;
        }
        if is_external_non_rust_tree(root, &child) {
            continue;
        }
        let metadata = fs::symlink_metadata(&child)
            .map_err(|error| format!("unable to inspect {}: {error}", child.display()))?;
        if metadata.file_type().is_symlink() {
            if child_name.eq_ignore_ascii_case(".cargo") || child.is_dir() {
                return Err(format!(
                    "symlink can redirect nested Cargo configuration: {}",
                    child.display()
                ));
            }
            continue;
        }
        if !metadata.is_dir() {
            continue;
        }
        if child_name.eq_ignore_ascii_case(".cargo") {
            if path == root && child_name == ".cargo" {
                continue;
            }
            if path == root {
                return Err(format!(
                    "case-variant root Cargo config directory is ambiguous on the primary macOS host: {}",
                    child.display()
                ));
            }
            for config_entry in fs::read_dir(&child)
                .map_err(|error| format!("unable to read {}: {error}", child.display()))?
            {
                let config_entry = config_entry
                    .map_err(|error| format!("unable to read Cargo config entry: {error}"))?;
                let config_name = config_entry.file_name();
                let config_name = config_name.to_string_lossy();
                if config_name.eq_ignore_ascii_case("config")
                    || config_name.eq_ignore_ascii_case("config.toml")
                {
                    return Err(format!(
                        "nested Cargo config can change production source semantics when a package is built directly: {}",
                        config_entry.path().display()
                    ));
                }
            }
        } else {
            reject_nested_cargo_configs(root, &child)?;
        }
    }
    Ok(())
}

fn rustflags_tokens(value: &toml::Value, context: &str) -> Result<Vec<String>, String> {
    let values: Vec<&str> = match value {
        toml::Value::String(value) => vec![value.as_str()],
        toml::Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| format!("{context} rustflags entries must be strings"))
            })
            .collect::<Result<_, _>>()?,
        _ => {
            return Err(format!(
                "{context} rustflags must be a string or string array"
            ))
        }
    };
    Ok(values
        .into_iter()
        .flat_map(str::split_ascii_whitespace)
        .map(str::to_owned)
        .collect())
}

fn reject_test_cfg_rustflags(value: &toml::Value, context: &str) -> Result<(), String> {
    let tokens = rustflags_tokens(value, context)?;
    for (index, token) in tokens.iter().enumerate() {
        if token.starts_with('@') {
            return Err(format!(
                "{context} rustflags use an unaudited response file: {token}"
            ));
        }
        let inline_cfg = token.strip_prefix("--cfg=");
        let following_cfg = (token == "--cfg")
            .then(|| tokens.get(index + 1).map(String::as_str))
            .flatten();
        if inline_cfg
            .or(following_cfg)
            .is_some_and(|cfg| cfg == "test" || cfg.starts_with("test="))
        {
            return Err(format!("{context} rustflags force cfg(test) in production"));
        }
    }
    Ok(())
}

fn validate_process_rust_environment() -> Result<(), String> {
    for (key, value) in env::vars() {
        let key_upper = key.to_ascii_uppercase();
        if matches!(
            key_upper.as_str(),
            "RUSTC"
                | "RUSTDOC"
                | "RUSTC_WRAPPER"
                | "RUSTC_WORKSPACE_WRAPPER"
                | "CARGO_BUILD_RUSTC"
                | "CARGO_BUILD_RUSTDOC"
                | "CARGO_BUILD_RUSTC_WRAPPER"
                | "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"
                | "CARGO_BUILD_TARGET"
        ) {
            return Err(format!(
                "process environment overrides audited Rust compiler semantics via {key}"
            ));
        }
        let is_rustflags = matches!(
            key_upper.as_str(),
            "RUSTFLAGS" | "CARGO_BUILD_RUSTFLAGS" | "CARGO_ENCODED_RUSTFLAGS"
        ) || (key_upper.starts_with("CARGO_TARGET_")
            && key_upper.ends_with("_RUSTFLAGS"));
        if key_upper.starts_with("CARGO_TARGET_")
            && (key_upper.ends_with("_LINKER") || key_upper.ends_with("_RUNNER"))
        {
            return Err(format!(
                "process environment can replace or skip the Rust risk gate via {key}"
            ));
        }
        if is_rustflags {
            let decoded = if key_upper == "CARGO_ENCODED_RUSTFLAGS" {
                value.replace('\u{1f}', " ")
            } else {
                value
            };
            reject_test_cfg_rustflags(
                &toml::Value::String(decoded),
                &format!("environment {key}"),
            )?;
        }
    }
    Ok(())
}

fn validate_cargo_config(root: &Path) -> Result<(), String> {
    for directory in AUDITED_SOURCE_DIRECTORIES
        .into_iter()
        .chain(TEST_ONLY_WORKSPACE_MEMBERS)
    {
        reject_nested_cargo_configs(root, &root.join(directory))?;
    }
    for relative in [".cargo/config", ".cargo/config.toml"] {
        let path = root.join(relative);
        if !path.exists() {
            continue;
        }
        validate_no_symlink_ancestors(root, &path)?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("unable to inspect {}: {error}", path.display()))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!(
                "Cargo config must be a regular repository file: {}",
                path.display()
            ));
        }
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
        let value: toml::Value = toml::from_str(&source)
            .map_err(|error| format!("unable to parse {}: {error}", path.display()))?;
        let table = value
            .as_table()
            .ok_or_else(|| format!("Cargo config is not a TOML table: {}", path.display()))?;
        if table.contains_key("paths") {
            return Err(format!(
                "Cargo path overrides can bypass audited source roots: {}",
                path.display()
            ));
        }
        if let Some(build) = table.get("build").and_then(toml::Value::as_table) {
            for key in ["rustc", "rustdoc", "rustc-workspace-wrapper", "target"] {
                if build.contains_key(key) {
                    return Err(format!(
                        "Cargo build.{key} can replace audited compiler semantics: {}",
                        path.display()
                    ));
                }
            }
            if let Some(wrapper) = build.get("rustc-wrapper") {
                if wrapper.as_str() != Some(RUSTC_WRAPPER_CONTRACTS[0].0) {
                    return Err(format!(
                        "Cargo build.rustc-wrapper is not the hash-contracted wrapper: {}",
                        path.display()
                    ));
                }
            }
            if let Some(rustflags) = build.get("rustflags") {
                reject_test_cfg_rustflags(rustflags, "Cargo build")?;
            }
        }
        if let Some(targets) = table.get("target").and_then(toml::Value::as_table) {
            for (target, settings) in targets {
                let settings = settings.as_table().ok_or_else(|| {
                    format!(
                        "Cargo target {target} config is not a table: {}",
                        path.display()
                    )
                })?;
                if let Some(rustflags) = settings.get("rustflags") {
                    reject_test_cfg_rustflags(rustflags, &format!("Cargo target {target}"))?;
                }
                if settings.contains_key("runner") {
                    return Err(format!(
                        "Cargo target {target} runner can skip the Rust risk gate: {}",
                        path.display()
                    ));
                }
                if settings.contains_key("linker") {
                    return Err(format!(
                        "Cargo target {target} linker can replace the Rust risk gate executable: {}",
                        path.display()
                    ));
                }
            }
        }
        if let Some(environment) = table.get("env").and_then(toml::Value::as_table) {
            for key in environment.keys() {
                let key_upper = key.to_ascii_uppercase();
                if matches!(
                    key_upper.as_str(),
                    "RUSTFLAGS"
                        | "CARGO_ENCODED_RUSTFLAGS"
                        | "CARGO_BUILD_RUSTFLAGS"
                        | "CARGO_BUILD_RUSTC"
                        | "CARGO_BUILD_RUSTDOC"
                        | "CARGO_BUILD_RUSTC_WRAPPER"
                        | "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"
                        | "CARGO_BUILD_TARGET"
                        | "RUSTC"
                        | "RUSTDOC"
                        | "RUSTC_WRAPPER"
                        | "RUSTC_WORKSPACE_WRAPPER"
                ) || (key_upper.starts_with("CARGO_TARGET_")
                    && (key_upper.ends_with("_LINKER")
                        || key_upper.ends_with("_RUSTFLAGS")
                        || key_upper.ends_with("_RUNNER")))
                {
                    return Err(format!(
                        "Cargo config environment overrides audited Rust compilation via {key}: {}",
                        path.display()
                    ));
                }
            }
        }
        if let Some(sources) = table.get("source").and_then(toml::Value::as_table) {
            for (name, source) in sources {
                let source = source.as_table().ok_or_else(|| {
                    format!("Cargo source {name} is not a table: {}", path.display())
                })?;
                if source.contains_key("directory") || source.contains_key("local-registry") {
                    return Err(format!(
                        "Cargo source {name} uses a local dependency override outside the source audit: {}",
                        path.display()
                    ));
                }
                if ["registry", "git"].iter().any(|key| {
                    source
                        .get(*key)
                        .and_then(toml::Value::as_str)
                        .is_some_and(is_local_file_url)
                }) {
                    return Err(format!(
                        "Cargo source {name} uses a file-backed registry or git source outside the source audit: {}",
                        path.display()
                    ));
                }
            }
        }
        if let Some(registries) = table.get("registries").and_then(toml::Value::as_table) {
            for (name, registry) in registries {
                if registry
                    .as_table()
                    .and_then(|registry| registry.get("index"))
                    .and_then(toml::Value::as_str)
                    .is_some_and(is_local_file_url)
                {
                    return Err(format!(
                        "Cargo registry {name} uses a file-backed index outside the source audit: {}",
                        path.display()
                    ));
                }
            }
        }
        if let Some(patches) = table.get("patch").and_then(toml::Value::as_table) {
            for (registry, patch_table) in patches {
                validate_dependency_table(
                    root,
                    root,
                    patch_table.as_table(),
                    &format!("Cargo config patch {registry}"),
                )?;
            }
        }
        validate_dependency_table(
            root,
            root,
            table.get("replace").and_then(toml::Value::as_table),
            "Cargo config replace",
        )?;
    }
    Ok(())
}

fn validate_hashed_file_contracts(
    root: &Path,
    mode: AuditMode,
    contracts: &[(&str, &str, &str)],
    label: &str,
) -> Result<(), String> {
    for (relative, current_hash, historical_hash) in contracts {
        let path = root.join(relative);
        validate_no_symlink_ancestors(root, &path)?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("unable to inspect contracted {label} {relative}: {error}"))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!(
                "contracted {label} is not a regular file: {relative}"
            ));
        }
        let bytes = fs::read(&path)
            .map_err(|error| format!("unable to read contracted {label} {relative}: {error}"))?;
        let actual = hex::encode(Sha256::digest(bytes));
        let expected = match mode {
            AuditMode::Current => current_hash,
            AuditMode::HistoricalReplay => historical_hash,
        };
        if actual != *expected {
            return Err(format!(
                "contracted {label} hash changed: {relative} expected={expected} actual={actual}"
            ));
        }
    }
    Ok(())
}

fn validate_current_only_build_tool_contracts(root: &Path, mode: AuditMode) -> Result<(), String> {
    for (relative, current_hash) in CURRENT_ONLY_BUILD_TOOL_CONTRACTS {
        let path = root.join(relative);
        if mode == AuditMode::HistoricalReplay {
            if path.exists() {
                return Err(format!(
                    "current-only Cargo build helper must be absent from historical replay: {relative}"
                ));
            }
            continue;
        }
        validate_no_symlink_ancestors(root, &path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!("unable to inspect current-only Cargo build helper {relative}: {error}")
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(format!(
                "current-only Cargo build helper is not a regular file: {relative}"
            ));
        }
        let bytes = fs::read(&path).map_err(|error| {
            format!("unable to read current-only Cargo build helper {relative}: {error}")
        })?;
        let actual = hex::encode(Sha256::digest(bytes));
        if actual != current_hash {
            return Err(format!(
                "current-only Cargo build helper hash changed: {relative} expected={current_hash} actual={actual}"
            ));
        }
    }
    Ok(())
}

fn validate_shared_build_directive_dependency(root: &Path, mode: AuditMode) -> Result<(), String> {
    if mode == AuditMode::HistoricalReplay {
        return Ok(());
    }

    let workspace_source = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|error| format!("unable to read workspace manifest: {error}"))?;
    let workspace: toml::Value = toml::from_str(&workspace_source)
        .map_err(|error| format!("unable to parse workspace manifest: {error}"))?;
    let workspace_table = workspace
        .as_table()
        .ok_or("workspace manifest is not a TOML table")?;
    let workspace_settings = workspace_table
        .get("workspace")
        .and_then(toml::Value::as_table)
        .ok_or("workspace manifest is missing [workspace]")?;
    let helper_member_count = workspace_settings
        .get("members")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|member| member.as_str() == Some("crates/cargo-build-directive"))
        .count();
    if helper_member_count != 1 {
        return Err(String::from(
            "shared Cargo build-directive helper must be exactly one workspace member",
        ));
    }
    let helper_dependency = workspace_settings
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .and_then(|dependencies| dependencies.get("cargo-build-directive"))
        .and_then(toml::Value::as_table)
        .ok_or("workspace cargo-build-directive dependency is missing or malformed")?;
    if helper_dependency.len() != 1
        || helper_dependency.get("path").and_then(toml::Value::as_str)
            != Some("crates/cargo-build-directive")
    {
        return Err(String::from(
            "workspace cargo-build-directive dependency must resolve only to crates/cargo-build-directive",
        ));
    }

    for relative_manifest in [
        "apps/pi4-driver-runtime/Cargo.toml",
        "apps/root-task/Cargo.toml",
        "crates/sel4-sys/Cargo.toml",
    ] {
        let source = fs::read_to_string(root.join(relative_manifest)).map_err(|error| {
            format!("unable to read build-script manifest {relative_manifest}: {error}")
        })?;
        let manifest: toml::Value = toml::from_str(&source).map_err(|error| {
            format!("unable to parse build-script manifest {relative_manifest}: {error}")
        })?;
        let dependency = manifest
            .as_table()
            .and_then(|table| table.get("build-dependencies"))
            .and_then(toml::Value::as_table)
            .and_then(|dependencies| dependencies.get("cargo-build-directive"))
            .and_then(toml::Value::as_table)
            .ok_or_else(|| {
                format!(
                    "{relative_manifest} must use the shared cargo-build-directive build dependency"
                )
            })?;
        if dependency.len() != 1
            || dependency.get("workspace").and_then(toml::Value::as_bool) != Some(true)
        {
            return Err(format!(
                "{relative_manifest} cargo-build-directive dependency must be exactly workspace = true"
            ));
        }
    }
    Ok(())
}

fn collect_external_build_modules(
    root: &Path,
    source_path: &Path,
    items: &[Item],
    nested_inline_module: bool,
    modules: &mut Vec<String>,
) -> Result<(), String> {
    for item in items {
        let Item::Mod(module) = item else {
            continue;
        };
        if let Some((_, nested_items)) = &module.content {
            collect_external_build_modules(root, source_path, nested_items, true, modules)?;
            continue;
        }
        if nested_inline_module {
            return Err(format!(
                "external modules nested inside build-script inline modules are not contractible: {}::{}",
                source_path.display(),
                module.ident
            ));
        }
        let mut declared_path = None;
        for attribute in &module.attrs {
            if !attribute.path().is_ident("path") {
                continue;
            }
            let Meta::NameValue(name_value) = &attribute.meta else {
                return Err(format!(
                    "build-script module path must be one string literal: {}::{}",
                    source_path.display(),
                    module.ident
                ));
            };
            let Expr::Lit(expression) = &name_value.value else {
                return Err(format!(
                    "build-script module path must be one string literal: {}::{}",
                    source_path.display(),
                    module.ident
                ));
            };
            let Lit::Str(path) = &expression.lit else {
                return Err(format!(
                    "build-script module path must be one string literal: {}::{}",
                    source_path.display(),
                    module.ident
                ));
            };
            if declared_path.replace(path.value()).is_some() {
                return Err(format!(
                    "build-script module has duplicate path attributes: {}::{}",
                    source_path.display(),
                    module.ident
                ));
            }
        }
        let declared_path = declared_path.ok_or_else(|| {
            format!(
                "external build-script modules require an explicit hash-contracted path: {}::{}",
                source_path.display(),
                module.ident
            )
        })?;
        let parent = source_path.parent().ok_or_else(|| {
            format!(
                "build-script source has no parent: {}",
                source_path.display()
            )
        })?;
        let path = normalize_lexical_path(&parent.join(declared_path))?;
        validate_no_symlink_ancestors(root, &path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "unable to inspect external build-script module {}: {error}",
                path.display()
            )
        })?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || path.extension().and_then(|extension| extension.to_str()) != Some("rs")
        {
            return Err(format!(
                "external build-script module is not a regular Rust file: {}",
                path.display()
            ));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| {
                format!(
                    "external build-script module escapes repository root: {}",
                    path.display()
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");
        modules.push(relative);
    }
    Ok(())
}

fn validate_build_tooling_contracts(root: &Path, mode: AuditMode) -> Result<(), String> {
    validate_hashed_file_contracts(root, mode, &BUILD_SCRIPT_CONTRACTS, "Cargo build script")?;
    validate_hashed_file_contracts(
        root,
        mode,
        &BUILD_SCRIPT_INPUT_CONTRACTS,
        "Cargo build-script module",
    )?;
    validate_hashed_file_contracts(root, mode, &RUSTC_WRAPPER_CONTRACTS, "rustc wrapper")?;
    validate_current_only_build_tool_contracts(root, mode)?;
    validate_shared_build_directive_dependency(root, mode)?;

    let mut actual_build_modules = Vec::new();
    for (relative, _, _) in BUILD_SCRIPT_CONTRACTS
        .iter()
        .chain(BUILD_SCRIPT_INPUT_CONTRACTS.iter())
    {
        let source_path = root.join(relative);
        let source = fs::read_to_string(&source_path).map_err(|error| {
            format!("unable to read contracted build-script source {relative}: {error}")
        })?;
        let syntax = syn::parse_file(&source).map_err(|error| {
            format!("unable to parse contracted build-script source {relative}: {error}")
        })?;
        collect_external_build_modules(
            root,
            &source_path,
            &syntax.items,
            false,
            &mut actual_build_modules,
        )?;
    }
    actual_build_modules.sort();
    actual_build_modules.dedup();
    let mut expected_build_modules: Vec<String> = BUILD_SCRIPT_INPUT_CONTRACTS
        .iter()
        .map(|(path, _, _)| (*path).to_owned())
        .collect();
    expected_build_modules.sort();
    if actual_build_modules != expected_build_modules {
        return Err(format!(
            "Cargo build-script module contracts must be exact: expected={expected_build_modules:?} actual={actual_build_modules:?}"
        ));
    }

    let mut rust_files = Vec::new();
    for directory in AUDITED_SOURCE_DIRECTORIES {
        collect_rust_files(root, &root.join(directory), &mut rust_files)?;
    }
    let mut actual_build_scripts: Vec<String> = rust_files
        .into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name == "build.rs"))
        .map(|path| {
            path.strip_prefix(root)
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .map_err(|_| {
                    format!(
                        "audited Cargo build script escaped repository root: {}",
                        path.display()
                    )
                })
        })
        .collect::<Result<_, _>>()?;
    actual_build_scripts.sort();
    let mut expected_build_scripts: Vec<String> = BUILD_SCRIPT_CONTRACTS
        .iter()
        .map(|(path, _, _)| (*path).to_owned())
        .collect();
    expected_build_scripts.sort();
    if actual_build_scripts != expected_build_scripts {
        return Err(format!(
            "Cargo build-script contracts must cover every production build.rs exactly: expected={expected_build_scripts:?} actual={actual_build_scripts:?}"
        ));
    }
    Ok(())
}

fn validate_manifests(root: &Path) -> Result<(), String> {
    validate_cargo_config(root)?;
    let workspace_manifest = root.join("Cargo.toml");
    let workspace_source = fs::read_to_string(&workspace_manifest).map_err(|error| {
        format!(
            "unable to read workspace manifest {}: {error}",
            workspace_manifest.display()
        )
    })?;
    let workspace: toml::Value = toml::from_str(&workspace_source).map_err(|error| {
        format!(
            "unable to parse workspace manifest {}: {error}",
            workspace_manifest.display()
        )
    })?;
    let workspace_table = workspace
        .as_table()
        .ok_or("workspace manifest is not a TOML table")?;
    if workspace_table.contains_key("package") {
        return Err(String::from(
            "the workspace root cannot also be a package outside audited source roots",
        ));
    }
    if let Some(members) = workspace_table
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|value| value.get("members"))
        .and_then(toml::Value::as_array)
    {
        for member in members {
            let member = member
                .as_str()
                .ok_or("Cargo workspace member must be a literal path")?;
            let normalized = normalize_lexical_path(&root.join(member))?;
            if is_in_audited_source_roots(root, &normalized) {
                validate_manifest_repo_path(
                    root,
                    root,
                    member,
                    "Cargo workspace production member",
                    false,
                )?;
            } else {
                let allowed_test_member = TEST_ONLY_WORKSPACE_MEMBERS
                    .iter()
                    .any(|allowed| normalized == root.join(allowed));
                if !allowed_test_member {
                    return Err(format!(
                        "workspace member is outside audited source roots and the tests-only contract: {member}"
                    ));
                }
                validate_no_symlink_ancestors(root, &normalized)?;
                let metadata = fs::symlink_metadata(&normalized).map_err(|error| {
                    format!("unable to inspect tests-only workspace member {member}: {error}")
                })?;
                if !metadata.is_dir() || !normalized.join("Cargo.toml").is_file() {
                    return Err(format!(
                        "tests-only workspace member is not a regular Cargo package: {member}"
                    ));
                }
            }
        }
    }
    validate_dependency_table(
        root,
        root,
        workspace_table
            .get("workspace")
            .and_then(toml::Value::as_table)
            .and_then(|value| value.get("dependencies"))
            .and_then(toml::Value::as_table),
        "Cargo workspace",
    )?;
    if let Some(patches) = workspace_table.get("patch").and_then(toml::Value::as_table) {
        for (registry, patch_table) in patches {
            validate_dependency_table(
                root,
                root,
                patch_table.as_table(),
                &format!("Cargo patch {registry}"),
            )?;
        }
    }
    validate_dependency_table(
        root,
        root,
        workspace_table
            .get("replace")
            .and_then(toml::Value::as_table),
        "Cargo replace",
    )?;

    let mut manifests = Vec::new();
    for directory in AUDITED_SOURCE_DIRECTORIES {
        collect_manifest_files(root, &root.join(directory), &mut manifests)?;
    }
    manifests.sort();
    for manifest in manifests {
        validate_manifest(root, &manifest)?;
    }
    Ok(())
}

fn validate_canonical_component_files(root: &Path) -> Result<(), String> {
    for relative in LINKED_RUNTIME_HAL_CANONICAL_FILES {
        let path = root.join(relative);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!("canonical risk component file missing: {relative}: {error}")
        })?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(format!(
                "canonical risk component path is not a regular file: {relative}"
            ));
        }
    }
    Ok(())
}

#[derive(Default)]
struct FilesAudit {
    counts: RiskCounts,
    out_dir_includes: Vec<(String, String)>,
}

fn count_files(root: &Path, files: Vec<PathBuf>, mode: AuditMode) -> Result<FilesAudit, String> {
    let mut result = FilesAudit::default();
    for path in files {
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
        let audit = audit_source(&source)
            .map_err(|error| format!("unable to parse {}: {error}", path.display()))?;
        let invalid_attributes: Vec<_> = audit
            .invalid_attributes
            .iter()
            .filter(|error| {
                !(mode.allows_legacy_include() && error.as_str() == UNQUALIFIED_INCLUDE_ERROR)
            })
            .cloned()
            .collect();
        if !invalid_attributes.is_empty() {
            return Err(format!(
                "invalid production Rust attributes: {}: {}",
                path.display(),
                invalid_attributes.join(", ")
            ));
        }
        for redirect in audit.path_redirects {
            validate_path_redirect(root, &path, &redirect)?;
        }
        for include in audit.include_expressions {
            if let Some(contract) = validate_include_expression(root, &path, &include, mode)? {
                result.out_dir_includes.push(contract);
            }
        }
        result.counts.add(audit.counts);
    }
    Ok(result)
}

fn validate_complete_out_dir_contracts(
    root: &Path,
    mode: AuditMode,
    includes: &[(String, String)],
) -> Result<(), String> {
    let mut actual = includes.to_vec();
    actual.sort();
    let mut expected: Vec<_> = OUT_DIR_INCLUDE_CONTRACTS
        .iter()
        .filter(|(source, _, _, _, _)| mode == AuditMode::Current || root.join(source).is_file())
        .map(|(source, output, _, _, _)| ((*source).to_owned(), (*output).to_owned()))
        .collect();
    expected.sort();
    if actual != expected {
        return Err(format!(
            "OUT_DIR include contracts must be exact and used once: expected={expected:?} actual={actual:?}"
        ));
    }
    Ok(())
}

fn count_tree(root: &Path, mode: AuditMode) -> Result<RiskCounts, String> {
    validate_canonical_component_files(root)?;
    validate_manifests(root)?;
    validate_build_tooling_contracts(root, mode)?;
    let mut files = Vec::new();
    for directory in AUDITED_SOURCE_DIRECTORIES {
        collect_rust_files(root, &root.join(directory), &mut files)?;
    }
    files.sort();
    let audit = count_files(root, files, mode)?;
    validate_complete_out_dir_contracts(root, mode, &audit.out_dir_includes)?;
    Ok(audit.counts)
}

fn count_paths(
    root: &Path,
    relative_paths: &[&str],
    mode: AuditMode,
) -> Result<RiskCounts, String> {
    let mut files = Vec::new();
    for relative_path in relative_paths {
        let path = root.join(relative_path);
        if path.is_dir() {
            collect_rust_files(root, &path, &mut files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        } else {
            return Err(format!(
                "risk component path is missing or not Rust: {}",
                path.display()
            ));
        }
    }
    files.sort();
    files.dedup();
    count_files(root, files, mode).map(|audit| audit.counts)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RiskMetadata {
    scanner: String,
    source_scope: String,
    historical_baseline_commit: String,
    historical_unsafe: usize,
    historical_unwrap: usize,
    historical_expect: usize,
    historical_panic: usize,
    accepted_unsafe_delta: usize,
    accepted_unwrap_delta: usize,
    accepted_expect_delta: usize,
    accepted_panic_delta: usize,
    linked_runtime_hal_historical_unsafe: usize,
    linked_runtime_hal_historical_unwrap: usize,
    linked_runtime_hal_historical_expect: usize,
    linked_runtime_hal_historical_panic: usize,
    outside_linked_runtime_hal_historical_unsafe: usize,
    outside_linked_runtime_hal_historical_unwrap: usize,
    outside_linked_runtime_hal_historical_expect: usize,
    outside_linked_runtime_hal_historical_panic: usize,
    linked_runtime_hal_paths: Vec<String>,
    external_non_rust_trees: Vec<String>,
    out_dir_generator_contracts: Vec<String>,
    build_script_contracts: Vec<String>,
    build_script_input_contracts: Vec<String>,
    current_only_build_tool_contracts: Vec<String>,
    rustc_wrapper_contracts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RiskBaselines {
    non_test: RiskCounts,
    linked_runtime_hal: RiskCounts,
    outside_linked_runtime_hal: RiskCounts,
    metadata: RiskMetadata,
}

fn read_baseline(path: &Path) -> Result<RiskBaselines, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    parse_baseline(&text)
}

fn parse_baseline(text: &str) -> Result<RiskBaselines, String> {
    let baseline: RiskBaselines =
        toml::from_str(text).map_err(|error| format!("invalid risk baseline TOML: {error}"))?;
    validate_baseline(&baseline)?;
    Ok(baseline)
}

fn validate_baseline(baseline: &RiskBaselines) -> Result<(), String> {
    if baseline.metadata.scanner != SCANNER_VERSION {
        return Err(format!(
            "risk baseline scanner must be {SCANNER_VERSION}, found {}",
            baseline.metadata.scanner
        ));
    }
    if baseline.metadata.historical_baseline_commit != HISTORICAL_BASELINE_COMMIT {
        return Err(format!(
            "risk baseline commit must be {HISTORICAL_BASELINE_COMMIT}, found {}",
            baseline.metadata.historical_baseline_commit
        ));
    }
    if baseline.metadata.source_scope != SOURCE_SCOPE {
        return Err(String::from(
            "risk baseline source_scope does not match the scanner's fail-closed source contract",
        ));
    }
    if baseline.metadata.historical_unsafe != HISTORICAL_GLOBAL_COUNTS.unsafe_count
        || baseline.metadata.historical_unwrap != HISTORICAL_GLOBAL_COUNTS.unwrap_count
        || baseline.metadata.historical_expect != HISTORICAL_GLOBAL_COUNTS.expect_count
        || baseline.metadata.historical_panic != HISTORICAL_GLOBAL_COUNTS.panic_count
        || baseline.metadata.linked_runtime_hal_historical_unsafe
            != HISTORICAL_LINKED_RUNTIME_HAL_COUNTS.unsafe_count
        || baseline.metadata.linked_runtime_hal_historical_unwrap
            != HISTORICAL_LINKED_RUNTIME_HAL_COUNTS.unwrap_count
        || baseline.metadata.linked_runtime_hal_historical_expect
            != HISTORICAL_LINKED_RUNTIME_HAL_COUNTS.expect_count
        || baseline.metadata.linked_runtime_hal_historical_panic
            != HISTORICAL_LINKED_RUNTIME_HAL_COUNTS.panic_count
        || baseline
            .metadata
            .outside_linked_runtime_hal_historical_unsafe
            != HISTORICAL_OUTSIDE_LINKED_RUNTIME_HAL_COUNTS.unsafe_count
        || baseline
            .metadata
            .outside_linked_runtime_hal_historical_unwrap
            != HISTORICAL_OUTSIDE_LINKED_RUNTIME_HAL_COUNTS.unwrap_count
        || baseline
            .metadata
            .outside_linked_runtime_hal_historical_expect
            != HISTORICAL_OUTSIDE_LINKED_RUNTIME_HAL_COUNTS.expect_count
        || baseline
            .metadata
            .outside_linked_runtime_hal_historical_panic
            != HISTORICAL_OUTSIDE_LINKED_RUNTIME_HAL_COUNTS.panic_count
        || baseline.metadata.accepted_unsafe_delta != 135
        || baseline.metadata.accepted_unwrap_delta != 0
        || baseline.metadata.accepted_expect_delta != 2
        || baseline.metadata.accepted_panic_delta != 6
    {
        return Err(String::from(
            "risk baseline historical v4 metadata must remain global=(693,38,240,96) linked=(146,0,2,0) outside=(547,38,238,96); approved 26e deltas must be unsafe=135 unwrap=0 expect=2 panic=6",
        ));
    }
    let expected_paths: Vec<String> = LINKED_RUNTIME_HAL_PATHS
        .iter()
        .map(|path| (*path).to_owned())
        .collect();
    if baseline.metadata.linked_runtime_hal_paths != expected_paths {
        return Err(String::from(
            "risk baseline linked_runtime_hal_paths do not match the scanner component boundary",
        ));
    }
    let expected_external_trees: Vec<String> = EXTERNAL_NON_RUST_TREES
        .iter()
        .map(|path| (*path).to_owned())
        .collect();
    if baseline.metadata.external_non_rust_trees != expected_external_trees {
        return Err(String::from(
            "risk baseline external_non_rust_trees do not match the scanner contract",
        ));
    }
    let expected_out_dir_contracts: Vec<String> = OUT_DIR_INCLUDE_CONTRACTS
        .iter()
        .map(
            |(source, output, generator, current_hash, historical_hash)| {
                format!("{source}:{output}:{generator}:{current_hash}:{historical_hash}")
            },
        )
        .collect();
    if baseline.metadata.out_dir_generator_contracts != expected_out_dir_contracts {
        return Err(String::from(
            "risk baseline OUT_DIR generator contracts do not match the scanner contract",
        ));
    }
    let expected_build_script_contracts: Vec<String> = BUILD_SCRIPT_CONTRACTS
        .iter()
        .map(|(path, current_hash, historical_hash)| {
            format!("{path}:{current_hash}:{historical_hash}")
        })
        .collect();
    if baseline.metadata.build_script_contracts != expected_build_script_contracts {
        return Err(String::from(
            "risk baseline Cargo build-script contracts do not match the scanner contract",
        ));
    }
    let expected_build_script_input_contracts: Vec<String> = BUILD_SCRIPT_INPUT_CONTRACTS
        .iter()
        .map(|(path, current_hash, historical_hash)| {
            format!("{path}:{current_hash}:{historical_hash}")
        })
        .collect();
    if baseline.metadata.build_script_input_contracts != expected_build_script_input_contracts {
        return Err(String::from(
            "risk baseline Cargo build-script input contracts do not match the scanner contract",
        ));
    }
    let expected_current_only_build_tool_contracts: Vec<String> = CURRENT_ONLY_BUILD_TOOL_CONTRACTS
        .iter()
        .map(|(path, current_hash)| {
            format!("{path}:{current_hash}:absent@{HISTORICAL_BASELINE_COMMIT}")
        })
        .collect();
    if baseline.metadata.current_only_build_tool_contracts
        != expected_current_only_build_tool_contracts
    {
        return Err(String::from(
            "risk baseline current-only Cargo build-tool contracts do not match the scanner contract",
        ));
    }
    let expected_rustc_wrapper_contracts: Vec<String> = RUSTC_WRAPPER_CONTRACTS
        .iter()
        .map(|(path, current_hash, historical_hash)| {
            format!("{path}:{current_hash}:{historical_hash}")
        })
        .collect();
    if baseline.metadata.rustc_wrapper_contracts != expected_rustc_wrapper_contracts {
        return Err(String::from(
            "risk baseline rustc-wrapper contracts do not match the scanner contract",
        ));
    }
    if baseline.non_test != ACTIVE_GLOBAL_CEILING
        || baseline.linked_runtime_hal != ACTIVE_LINKED_RUNTIME_HAL_CEILING
        || baseline.outside_linked_runtime_hal != ACTIVE_OUTSIDE_LINKED_RUNTIME_HAL_CEILING
    {
        return Err(String::from(
            "risk baseline ceilings must exactly match the approved v5 active ceilings",
        ));
    }
    let expected_outside = baseline
        .non_test
        .checked_sub(baseline.linked_runtime_hal)
        .ok_or("linked runtime/HAL baseline exceeds the global baseline")?;
    if baseline.outside_linked_runtime_hal != expected_outside {
        return Err(String::from(
            "outside linked-runtime/HAL baseline must equal global minus component counts",
        ));
    }
    Ok(())
}

fn print_counts(label: &str, counts: RiskCounts, baseline: Option<RiskCounts>) {
    println!("rust-risk-ratchet {label} counts:");
    for key in ["expect", "panic", "unsafe", "unwrap"] {
        let current = counts.value(key).unwrap_or_default();
        if let Some(baseline) = baseline {
            println!(
                "  - {key}: baseline={} current={current}",
                baseline.value(key).unwrap_or_default()
            );
        } else {
            println!("  - {key}: current={current}");
        }
    }
}

fn enforce_budgets(
    global: RiskCounts,
    linked_runtime_hal: RiskCounts,
    outside_linked_runtime_hal: RiskCounts,
    baseline: &RiskBaselines,
) -> Result<(), String> {
    let mut increases = Vec::new();
    for (label, current, ceiling) in [
        ("global", global, baseline.non_test),
        (
            "linked-runtime-hal",
            linked_runtime_hal,
            baseline.linked_runtime_hal,
        ),
        (
            "outside-linked-runtime-hal",
            outside_linked_runtime_hal,
            baseline.outside_linked_runtime_hal,
        ),
    ] {
        for key in ["unsafe", "unwrap", "expect", "panic"] {
            let current = current.value(key).unwrap_or_default();
            let ceiling = ceiling.value(key).unwrap_or_default();
            if current > ceiling {
                increases.push(format!(
                    "{label} non-test {key} count increased: baseline={ceiling} current={current}"
                ));
            }
        }
    }
    if increases.is_empty() {
        return Ok(());
    }
    let mut message = String::from("rust-risk-ratchet failed:\n");
    for increase in increases {
        message.push_str("  - ");
        message.push_str(&increase);
        message.push('\n');
    }
    Err(message.trim_end().to_owned())
}

fn configure_isolated_git_command(
    command: &mut Command,
    root: &Path,
    arguments: &[&str],
) -> Result<(), String> {
    let path = env::var_os("PATH")
        .ok_or("PATH is required to execute isolated historical-replay Git checks")?;
    command
        .env_clear()
        .env("PATH", path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_WORK_TREE", root)
        .env("LC_ALL", "C")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.untrackedCache=false")
        .arg("-C")
        .arg(root)
        .args(arguments);
    Ok(())
}

fn isolated_git_bytes(
    mut command: Command,
    root: &Path,
    arguments: &[&str],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    configure_isolated_git_command(&mut command, root, arguments)?;
    let output = if let Some(input) = input {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("unable to run git for historical replay: {error}"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or("unable to open Git historical-replay stdin")?;
        stdin
            .write_all(input)
            .map_err(|error| format!("unable to write Git historical-replay input: {error}"))?;
        drop(stdin);
        child
            .wait_with_output()
            .map_err(|error| format!("unable to wait for historical-replay Git: {error}"))?
    } else {
        command
            .output()
            .map_err(|error| format!("unable to run git for historical replay: {error}"))?
    };
    if !output.status.success() {
        return Err(format!(
            "git historical replay check failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

fn isolated_git_output(
    command: Command,
    root: &Path,
    arguments: &[&str],
) -> Result<String, String> {
    let output = isolated_git_bytes(command, root, arguments, None)?;
    String::from_utf8(output)
        .map(|value| value.trim().to_owned())
        .map_err(|_| String::from("git historical replay output is not UTF-8"))
}

fn git_bytes(root: &Path, arguments: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
    isolated_git_bytes(Command::new("git"), root, arguments, input)
}

fn git_output(root: &Path, arguments: &[&str]) -> Result<String, String> {
    isolated_git_output(Command::new("git"), root, arguments)
}

#[derive(Debug)]
struct HistoricalTreeEntry {
    mode: String,
    oid: String,
    path: String,
}

fn historical_tree_entries(root: &Path) -> Result<Vec<HistoricalTreeEntry>, String> {
    let mut arguments = vec!["ls-tree", "-r", "-z", "--full-tree", "HEAD", "--"];
    arguments.extend(HISTORICAL_ATTESTED_DIRECTORIES);
    arguments.extend(HISTORICAL_ATTESTED_FILES);
    let output = git_bytes(root, &arguments, None)?;
    let mut entries = Vec::new();
    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or("historical HEAD tree record is missing its path separator")?;
        let header = std::str::from_utf8(&record[..tab])
            .map_err(|_| "historical HEAD tree metadata is not UTF-8")?;
        let path = std::str::from_utf8(&record[tab + 1..])
            .map_err(|_| "historical HEAD contains a non-UTF-8 attested path")?;
        let mut fields = header.split_ascii_whitespace();
        let mode = fields
            .next()
            .ok_or("historical HEAD tree mode is missing")?;
        let kind = fields
            .next()
            .ok_or("historical HEAD tree object kind is missing")?;
        let oid = fields
            .next()
            .ok_or("historical HEAD tree object ID is missing")?;
        if fields.next().is_some() || kind != "blob" {
            return Err(format!(
                "historical attested path is not one blob object: {path}"
            ));
        }
        if !Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err(format!(
                "historical HEAD contains a non-canonical path: {path}"
            ));
        }
        entries.push(HistoricalTreeEntry {
            mode: mode.to_owned(),
            oid: oid.to_owned(),
            path: path.to_owned(),
        });
    }
    Ok(entries)
}

fn collect_attested_worktree_paths(
    root: &Path,
    path: &Path,
    paths: &mut HashSet<String>,
) -> Result<(), String> {
    if !path.exists() && fs::symlink_metadata(path).is_err() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "unable to inspect attested path {}: {error}",
            path.display()
        )
    })?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        for entry in fs::read_dir(path)
            .map_err(|error| format!("unable to enumerate {}: {error}", path.display()))?
        {
            let entry = entry
                .map_err(|error| format!("unable to enumerate {}: {error}", path.display()))?;
            collect_attested_worktree_paths(root, &entry.path(), paths)?;
        }
        return Ok(());
    }
    if !metadata.is_file() && !metadata.file_type().is_symlink() {
        return Err(format!(
            "historical attested path is not a regular file or symlink: {}",
            path.display()
        ));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("attested path escaped historical root: {}", path.display()))?;
    let relative = relative.to_str().ok_or_else(|| {
        format!(
            "historical attested path is not exact UTF-8: {}",
            path.display()
        )
    })?;
    paths.insert(relative.to_owned());
    Ok(())
}

fn historical_blob_contents(
    root: &Path,
    entries: &[HistoricalTreeEntry],
) -> Result<Vec<Vec<u8>>, String> {
    let mut request = String::new();
    for entry in entries {
        request.push_str(&entry.oid);
        request.push('\n');
    }
    let output = git_bytes(root, &["cat-file", "--batch"], Some(request.as_bytes()))?;
    let mut cursor = 0usize;
    let mut contents = Vec::with_capacity(entries.len());
    for entry in entries {
        let header_end = output[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| cursor + offset)
            .ok_or_else(|| format!("cat-file response header is missing for {}", entry.path))?;
        let header = std::str::from_utf8(&output[cursor..header_end])
            .map_err(|_| format!("cat-file response header is not UTF-8 for {}", entry.path))?;
        let mut fields = header.split_ascii_whitespace();
        let oid = fields.next().unwrap_or_default();
        let kind = fields.next().unwrap_or_default();
        let size = fields
            .next()
            .ok_or_else(|| format!("cat-file response size is missing for {}", entry.path))?
            .parse::<usize>()
            .map_err(|_| format!("cat-file response size is invalid for {}", entry.path))?;
        if fields.next().is_some() || oid != entry.oid || kind != "blob" {
            return Err(format!(
                "cat-file response does not match historical blob {}",
                entry.path
            ));
        }
        let body_start = header_end + 1;
        let body_end = body_start
            .checked_add(size)
            .ok_or("cat-file blob length overflow")?;
        if body_end >= output.len() || output[body_end] != b'\n' {
            return Err(format!(
                "cat-file response body is truncated for {}",
                entry.path
            ));
        }
        contents.push(output[body_start..body_end].to_vec());
        cursor = body_end + 1;
    }
    if cursor != output.len() {
        return Err(String::from(
            "cat-file returned trailing data after historical attestation",
        ));
    }
    Ok(contents)
}

#[cfg(unix)]
fn verify_historical_attested_files(root: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;

    let entries = historical_tree_entries(root)?;
    let tree_paths: HashSet<String> = entries.iter().map(|entry| entry.path.clone()).collect();
    if tree_paths.len() != entries.len() {
        return Err(String::from(
            "historical HEAD contains duplicate attested paths",
        ));
    }
    let mut worktree_paths = HashSet::new();
    for relative in HISTORICAL_ATTESTED_DIRECTORIES {
        collect_attested_worktree_paths(root, &root.join(relative), &mut worktree_paths)?;
    }
    for relative in HISTORICAL_ATTESTED_FILES {
        collect_attested_worktree_paths(root, &root.join(relative), &mut worktree_paths)?;
    }
    if tree_paths != worktree_paths {
        let mut missing: Vec<&String> = tree_paths.difference(&worktree_paths).collect();
        let mut extra: Vec<&String> = worktree_paths.difference(&tree_paths).collect();
        missing.sort();
        extra.sort();
        return Err(format!(
            "historical attested path set differs from raw HEAD: missing={:?} extra={:?}",
            missing.into_iter().take(8).collect::<Vec<_>>(),
            extra.into_iter().take(8).collect::<Vec<_>>()
        ));
    }

    let blobs = historical_blob_contents(root, &entries)?;
    for (entry, expected) in entries.iter().zip(blobs) {
        let path = root.join(&entry.path);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!("unable to inspect historical path {}: {error}", entry.path)
        })?;
        let actual = match entry.mode.as_str() {
            "100644" => {
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.permissions().mode() & 0o111 != 0
                {
                    return Err(format!(
                        "historical path mode/type differs from raw HEAD: {} expected=100644",
                        entry.path
                    ));
                }
                fs::read(&path).map_err(|error| {
                    format!("unable to read historical path {}: {error}", entry.path)
                })?
            }
            "100755" => {
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.permissions().mode() & 0o111 == 0
                {
                    return Err(format!(
                        "historical path mode/type differs from raw HEAD: {} expected=100755",
                        entry.path
                    ));
                }
                fs::read(&path).map_err(|error| {
                    format!("unable to read historical path {}: {error}", entry.path)
                })?
            }
            "120000" => {
                if !metadata.file_type().is_symlink() {
                    return Err(format!(
                        "historical path mode/type differs from raw HEAD: {} expected=120000",
                        entry.path
                    ));
                }
                fs::read_link(&path)
                    .map_err(|error| {
                        format!("unable to read historical symlink {}: {error}", entry.path)
                    })?
                    .as_os_str()
                    .as_bytes()
                    .to_vec()
            }
            mode => {
                return Err(format!(
                    "historical attested path has unsupported raw HEAD mode {mode}: {}",
                    entry.path
                ))
            }
        };
        if actual != expected {
            return Err(format!(
                "historical path bytes differ from raw HEAD blob: {}",
                entry.path
            ));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_historical_attested_files(_root: &Path) -> Result<(), String> {
    Err(String::from(
        "historical raw file/mode attestation requires a Unix host",
    ))
}

fn verify_historical_replay_root(root: &Path) -> Result<(), String> {
    let commit = git_output(root, &["rev-parse", "HEAD"])?;
    if commit != HISTORICAL_BASELINE_FULL_COMMIT {
        return Err(format!(
            "historical replay root must be exact commit {HISTORICAL_BASELINE_FULL_COMMIT}, found {commit}"
        ));
    }
    verify_historical_attested_files(root)
}

fn enforce_historical_replay_counts(
    global: RiskCounts,
    linked_runtime_hal: RiskCounts,
    outside_linked_runtime_hal: RiskCounts,
) -> Result<(), String> {
    if global != HISTORICAL_GLOBAL_COUNTS
        || linked_runtime_hal != HISTORICAL_LINKED_RUNTIME_HAL_COUNTS
        || outside_linked_runtime_hal != HISTORICAL_OUTSIDE_LINKED_RUNTIME_HAL_COUNTS
    {
        return Err(format!(
            "historical v4 replay drift: expected global={HISTORICAL_GLOBAL_COUNTS:?} linked={HISTORICAL_LINKED_RUNTIME_HAL_COUNTS:?} outside={HISTORICAL_OUTSIDE_LINKED_RUNTIME_HAL_COUNTS:?}; found global={global:?} linked={linked_runtime_hal:?} outside={outside_linked_runtime_hal:?}"
        ));
    }
    Ok(())
}

fn run() -> Result<(), String> {
    validate_process_rust_environment()?;
    let mut root = PathBuf::from(".");
    let mut baseline = Some(PathBuf::from("docs/audit/rust_risk_baseline.toml"));
    let mut baseline_explicit = false;
    let mut mode = AuditMode::Current;
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--root" => {
                root = PathBuf::from(arguments.next().ok_or("--root requires a path")?);
            }
            "--baseline" => {
                baseline_explicit = true;
                baseline = Some(PathBuf::from(
                    arguments.next().ok_or("--baseline requires a path")?,
                ));
            }
            "--counts-only" => baseline = None,
            "--historical-replay" => {
                mode = AuditMode::HistoricalReplay;
                baseline = None;
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }

    root = fs::canonicalize(&root)
        .map_err(|error| format!("unable to resolve audit root {}: {error}", root.display()))?;
    if mode == AuditMode::HistoricalReplay {
        if baseline_explicit {
            return Err(String::from(
                "--historical-replay cannot enforce a current baseline",
            ));
        }
        verify_historical_replay_root(&root)?;
    }
    let counts = count_tree(&root, mode)?;
    let linked_runtime_hal_counts = count_paths(&root, &LINKED_RUNTIME_HAL_PATHS, mode)?;
    let outside_linked_runtime_hal_counts = counts
        .checked_sub(linked_runtime_hal_counts)
        .ok_or("linked runtime/HAL counts exceed global counts")?;
    if mode == AuditMode::HistoricalReplay {
        enforce_historical_replay_counts(
            counts,
            linked_runtime_hal_counts,
            outside_linked_runtime_hal_counts,
        )?;
    }
    let Some(baseline_path) = baseline else {
        print_counts("global", counts, None);
        print_counts("linked-runtime-hal", linked_runtime_hal_counts, None);
        print_counts(
            "outside-linked-runtime-hal",
            outside_linked_runtime_hal_counts,
            None,
        );
        if mode == AuditMode::HistoricalReplay {
            println!("rust-risk historical replay passed");
        }
        return Ok(());
    };
    let baseline_path = if baseline_path.is_absolute() {
        baseline_path
    } else {
        root.join(baseline_path)
    };
    let baseline_values = read_baseline(&baseline_path)?;
    print_counts("global", counts, Some(baseline_values.non_test));
    print_counts(
        "linked-runtime-hal",
        linked_runtime_hal_counts,
        Some(baseline_values.linked_runtime_hal),
    );
    print_counts(
        "outside-linked-runtime-hal",
        outside_linked_runtime_hal_counts,
        Some(baseline_values.outside_linked_runtime_hal),
    );
    enforce_budgets(
        counts,
        linked_runtime_hal_counts,
        outside_linked_runtime_hal_counts,
        &baseline_values,
    )?;

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
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{
        audit_source, count_paths, count_source, count_tree, enforce_budgets,
        enforce_historical_replay_counts, eval_cfg_with_test_disabled, git_output,
        is_excluded_test_path, isolated_git_output, parse_baseline, validate_cargo_config,
        validate_current_only_build_tool_contracts, validate_include_expression, validate_manifest,
        validate_manifests, validate_path_redirect, verify_historical_attested_files, AuditMode,
        CfgValue, RiskCounts, ACTIVE_GLOBAL_CEILING, ACTIVE_LINKED_RUNTIME_HAL_CEILING,
        ACTIVE_OUTSIDE_LINKED_RUNTIME_HAL_CEILING, HISTORICAL_GLOBAL_COUNTS,
        HISTORICAL_LINKED_RUNTIME_HAL_COUNTS, HISTORICAL_OUTSIDE_LINKED_RUNTIME_HAL_COUNTS,
        LINKED_RUNTIME_HAL_PATHS,
    };

    const COMPLETE_BASELINE: &str = include_str!("../../../docs/audit/rust_risk_baseline.toml");
    static TEMP_REPO_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct TempRepo {
        root: PathBuf,
    }

    impl TempRepo {
        fn new() -> Self {
            let sequence = TEMP_REPO_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "cohesix-rust-risk-audit-{}-{sequence}",
                std::process::id()
            ));
            if root.exists() {
                fs::remove_dir_all(&root).expect("stale audit fixture is removable");
            }
            fs::create_dir_all(root.join("apps/example/src"))
                .expect("fixture app source directory is creatable");
            fs::create_dir_all(root.join("apps/example/tests"))
                .expect("fixture app tests directory is creatable");
            fs::create_dir_all(root.join("crates")).expect("fixture crates directory is creatable");
            fs::create_dir_all(root.join("tools")).expect("fixture tools directory is creatable");
            fs::write(
                root.join("Cargo.toml"),
                "[workspace]\nmembers = [\"apps/example\"]\nresolver = \"2\"\n",
            )
            .expect("fixture workspace manifest is writable");
            fs::write(
                root.join("apps/example/Cargo.toml"),
                "[package]\nname = \"example\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            )
            .expect("fixture package manifest is writable");
            fs::write(root.join("apps/example/src/lib.rs"), "pub fn safe() {}\n")
                .expect("fixture library is writable");
            Self {
                root: fs::canonicalize(root).expect("fixture root is canonicalizable"),
            }
        }

        fn source(&self) -> PathBuf {
            self.root.join("apps/example/src/lib.rs")
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).expect("audit fixture is removable");
        }
    }

    fn initialize_fixture_repository(repo: &TempRepo) {
        let path = std::env::var_os("PATH").expect("fixture Git execution requires PATH");
        let output = Command::new("git")
            .env_clear()
            .env("PATH", path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("LC_ALL", "C")
            .args(["init", "--quiet"])
            .arg(&repo.root)
            .output()
            .expect("fixture Git initializes");
        assert!(
            output.status.success(),
            "fixture repository initialization failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit_fixture_repository(repo: &TempRepo, message: &str) -> String {
        git_output(&repo.root, &["add", "--all"]).expect("fixture files are staged");
        git_output(
            &repo.root,
            &[
                "-c",
                "user.name=Audit Fixture",
                "-c",
                "user.email=audit@example.invalid",
                "commit",
                "-m",
                message,
            ],
        )
        .expect("fixture commit succeeds");
        git_output(&repo.root, &["rev-parse", "HEAD"]).expect("fixture HEAD resolves")
    }

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
    fn current_only_build_helper_hash_and_historical_absence_are_enforced() {
        let repo = TempRepo::new();
        let helper_root = repo.root.join("crates/cargo-build-directive");
        fs::create_dir_all(helper_root.join("src"))
            .expect("build helper fixture directory is creatable");
        fs::write(
            helper_root.join("Cargo.toml"),
            include_bytes!("../../../crates/cargo-build-directive/Cargo.toml"),
        )
        .expect("build helper manifest fixture is writable");
        fs::write(
            helper_root.join("src/lib.rs"),
            include_bytes!("../../../crates/cargo-build-directive/src/lib.rs"),
        )
        .expect("build helper source fixture is writable");

        validate_current_only_build_tool_contracts(&repo.root, AuditMode::Current)
            .expect("exact current helper passes its hash contract");
        assert!(validate_current_only_build_tool_contracts(
            &repo.root,
            AuditMode::HistoricalReplay
        )
        .is_err());

        fs::write(
            helper_root.join("src/lib.rs"),
            "pub fn emit_cargo_directive(value: String) { println!(\"{value}\"); }\n",
        )
        .expect("tampered build helper fixture is writable");
        assert!(
            validate_current_only_build_tool_contracts(&repo.root, AuditMode::Current).is_err()
        );
    }

    #[test]
    fn historical_git_checks_ignore_injected_repository_binding() {
        let expected = TempRepo::new();
        let hostile = TempRepo::new();
        initialize_fixture_repository(&expected);
        initialize_fixture_repository(&hostile);

        let mut command = Command::new("git");
        command.env("GIT_DIR", hostile.root.join(".git"));
        command.env("GIT_WORK_TREE", &hostile.root);
        let observed =
            isolated_git_output(command, &expected.root, &["rev-parse", "--show-toplevel"])
                .expect("isolated Git query succeeds");

        assert_eq!(
            fs::canonicalize(observed).expect("Git top-level path is canonicalizable"),
            expected.root
        );
    }

    #[cfg(unix)]
    #[test]
    fn raw_historical_attestation_accepts_exact_head_files() {
        let repo = TempRepo::new();
        initialize_fixture_repository(&repo);
        commit_fixture_repository(&repo, "baseline");

        verify_historical_attested_files(&repo.root)
            .expect("exact raw HEAD scanner inputs pass historical attestation");
    }

    #[cfg(unix)]
    #[test]
    fn raw_historical_attestation_ignores_redirected_core_worktree() {
        let expected = TempRepo::new();
        let hostile = TempRepo::new();
        initialize_fixture_repository(&expected);
        commit_fixture_repository(&expected, "baseline");
        git_output(
            &expected.root,
            &[
                "config",
                "core.worktree",
                hostile
                    .root
                    .to_str()
                    .expect("hostile fixture path is UTF-8"),
            ],
        )
        .expect("hostile core.worktree is configurable");
        fs::write(expected.source(), "pub fn changed() {}\n")
            .expect("the attested worktree is mutable");

        assert!(verify_historical_attested_files(&expected.root).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn raw_historical_attestation_ignores_replacement_refs() {
        let repo = TempRepo::new();
        initialize_fixture_repository(&repo);
        let baseline = commit_fixture_repository(&repo, "baseline");
        fs::write(repo.source(), "pub fn replacement() {}\n")
            .expect("replacement source is writable");
        let replacement = commit_fixture_repository(&repo, "replacement");

        git_output(&repo.root, &["reset", "--hard", &baseline])
            .expect("fixture resets to baseline");
        git_output(&repo.root, &["read-tree", &replacement])
            .expect("replacement tree is loaded into the fixture index");
        git_output(&repo.root, &["checkout-index", "--all", "--force"])
            .expect("replacement tree is materialized without moving HEAD");
        git_output(&repo.root, &["replace", &baseline, &replacement])
            .expect("hostile replacement ref is installed");

        assert!(verify_historical_attested_files(&repo.root).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn raw_historical_attestation_ignores_clean_filters() {
        let repo = TempRepo::new();
        initialize_fixture_repository(&repo);
        commit_fixture_repository(&repo, "baseline");
        fs::write(
            repo.root.join(".git/info/attributes"),
            "apps/example/src/lib.rs filter=hide\n",
        )
        .expect("hostile attributes file is writable");
        git_output(
            &repo.root,
            &["config", "filter.hide.clean", "git show HEAD:%f"],
        )
        .expect("hostile clean filter is configurable");
        git_output(&repo.root, &["config", "filter.hide.required", "true"])
            .expect("hostile clean filter is required");
        fs::write(repo.source(), "pub fn hidden_by_filter() {}\n")
            .expect("filtered source is writable");

        assert!(verify_historical_attested_files(&repo.root).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn raw_historical_attestation_rejects_ignored_extra_source() {
        let repo = TempRepo::new();
        initialize_fixture_repository(&repo);
        commit_fixture_repository(&repo, "baseline");
        fs::write(
            repo.root.join(".git/info/exclude"),
            "apps/example/src/hidden.rs\n",
        )
        .expect("fixture exclude file is writable");
        fs::write(
            repo.root.join("apps/example/src/hidden.rs"),
            "pub unsafe fn hidden() {}\n",
        )
        .expect("ignored extra source is writable");

        assert!(verify_historical_attested_files(&repo.root).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn raw_historical_attestation_rejects_backslash_path_collision() {
        let repo = TempRepo::new();
        initialize_fixture_repository(&repo);
        commit_fixture_repository(&repo, "baseline");
        fs::write(
            repo.root.join("apps/example\\src\\lib.rs"),
            "pub unsafe fn hidden() {}\n",
        )
        .expect("backslash-collision source is writable");

        assert!(verify_historical_attested_files(&repo.root).is_err());
    }

    #[test]
    fn source_counter_excludes_inline_test_only_items() {
        let source = r#"
            #[unsafe(no_mangle)]
            static EXPORTED: u8 = 1;

            fn production(value: Option<u8>, result: Result<u8, ()>) {
                let _ = value.unwrap();
                let _ = Option::unwrap(value);
                let _ = Result::expect(result, "qualified result");
                unsafe { core::ptr::read_volatile(&0u8); }
                core::panic!("production marker");
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
                unsafe_count: 3,
                unwrap_count: 2,
                expect_count: 2,
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
                    Option::unwrap($value);
                    Result::expect($value, "qualified macro value");
                    core::panic!("macro marker");
                }};
            }
        "#;

        assert_eq!(
            count_source(source).expect("macro source parses"),
            RiskCounts {
                unsafe_count: 1,
                unwrap_count: 1,
                expect_count: 2,
                panic_count: 1,
            }
        );
    }

    #[test]
    fn source_counter_handles_cfg_attr_raw_identifiers_and_macro_indirection() {
        let source = r#"
            #[cfg_attr(not(test), unsafe(no_mangle))]
            static EXPORTED: u8 = 1;

            fn raw_identifiers(value: Option<u8>, result: Result<u8, ()>) {
                let _ = value.r#unwrap();
                let _ = result.r#expect("raw result");
                r#panic!("raw panic");
            }

            invoke_method!(unwrap, value);
            invoke_method!(expect, result);
            invoke_macro!(panic);
        "#;

        assert_eq!(
            count_source(source).expect("source parses"),
            RiskCounts {
                unsafe_count: 1,
                unwrap_count: 2,
                expect_count: 2,
                panic_count: 2,
            }
        );
    }

    #[test]
    fn source_counter_resolves_imported_risk_aliases() {
        let source = r#"
            use core::include as load;
            use core::panic as abort;
            use Option::unwrap as extract;
            use Result::expect as require;

            fn aliases(value: Option<u8>, result: Result<u8, ()>) {
                let _ = extract(value);
                let _ = require(result, "required");
                abort!("aliased panic");
            }

            load!("generated.rs");
        "#;
        let audit = audit_source(source).expect("alias source parses");

        assert_eq!(audit.counts.unwrap_count, 1);
        assert_eq!(audit.counts.expect_count, 1);
        assert_eq!(audit.counts.panic_count, 1);
        assert_eq!(audit.include_expressions, ["\"generated.rs\""]);
        assert_eq!(audit.invalid_attributes.len(), 5);
    }

    #[test]
    fn baseline_is_typed_complete_and_rejects_duplicate_keys() {
        let parsed = parse_baseline(COMPLETE_BASELINE).expect("complete component baseline parses");
        assert_eq!(parsed.non_test, ACTIVE_GLOBAL_CEILING);
        assert_eq!(parsed.linked_runtime_hal, ACTIVE_LINKED_RUNTIME_HAL_CEILING);
        assert_eq!(
            parsed.outside_linked_runtime_hal,
            ACTIVE_OUTSIDE_LINKED_RUNTIME_HAL_CEILING
        );

        let duplicate = COMPLETE_BASELINE.replacen("unsafe = 828", "unsafe = 828\nunsafe = 828", 1);
        assert!(parse_baseline(&duplicate).is_err());
    }

    #[test]
    fn every_active_ceiling_rejects_baseline_inflation() {
        for section in [
            "non_test",
            "linked_runtime_hal",
            "outside_linked_runtime_hal",
        ] {
            for metric in ["unsafe", "unwrap", "expect", "panic"] {
                let mut value: toml::Value =
                    toml::from_str(COMPLETE_BASELINE).expect("baseline TOML parses");
                let current = value[section][metric]
                    .as_integer()
                    .expect("ceiling is an integer");
                value[section][metric] = toml::Value::Integer(current + 1);
                let inflated = toml::to_string(&value).expect("inflated TOML renders");
                assert!(
                    parse_baseline(&inflated).is_err(),
                    "{section}.{metric} inflation must fail"
                );
            }
        }
    }

    #[test]
    fn outside_component_budget_rejects_risk_relocation() {
        let baseline = parse_baseline(COMPLETE_BASELINE).expect("baseline parses");
        let global = baseline.non_test;
        let linked = RiskCounts {
            unsafe_count: baseline.linked_runtime_hal.unsafe_count - 1,
            ..baseline.linked_runtime_hal
        };
        let outside = global
            .checked_sub(linked)
            .expect("component remains within global counts");

        assert!(enforce_budgets(global, linked, outside, &baseline).is_err());
    }

    #[test]
    fn historical_replay_requires_every_exact_component_count() {
        assert!(enforce_historical_replay_counts(
            HISTORICAL_GLOBAL_COUNTS,
            HISTORICAL_LINKED_RUNTIME_HAL_COUNTS,
            HISTORICAL_OUTSIDE_LINKED_RUNTIME_HAL_COUNTS,
        )
        .is_ok());

        let drifted_outside = RiskCounts {
            expect_count: HISTORICAL_OUTSIDE_LINKED_RUNTIME_HAL_COUNTS.expect_count + 1,
            ..HISTORICAL_OUTSIDE_LINKED_RUNTIME_HAL_COUNTS
        };
        assert!(enforce_historical_replay_counts(
            HISTORICAL_GLOBAL_COUNTS,
            HISTORICAL_LINKED_RUNTIME_HAL_COUNTS,
            drifted_outside,
        )
        .is_err());
    }

    #[test]
    fn only_package_integration_tests_are_excluded() {
        let repo = TempRepo::new();
        let root = repo.root.as_path();
        assert!(is_excluded_test_path(
            root,
            &repo.root.join("apps/example/tests/fixture.rs")
        ));
        assert!(!is_excluded_test_path(
            root,
            &repo.root.join("apps/example/src/bootstrap/tests/cspace.rs")
        ));
        assert!(!is_excluded_test_path(
            root,
            &repo.root.join("apps/example/src/tests/runtime.rs")
        ));
        assert!(!is_excluded_test_path(
            root,
            &repo.root.join("apps/example/src/runtime_test.rs")
        ));
    }

    #[test]
    fn production_source_cannot_redirect_into_excluded_tests() {
        let audit = audit_source(
            r#"
                #[path = "../tests/hidden.rs"]
                mod hidden;
                ::core::include!(::core::concat!("../", "tests/also_hidden.rs"));
            "#,
        )
        .expect("redirect fixture parses");

        assert_eq!(audit.path_redirects, ["../tests/hidden.rs"]);
        assert_eq!(audit.include_expressions.len(), 1);
        assert!(audit.include_expressions[0].contains("tests/also_hidden.rs"));
    }

    #[test]
    fn split_concat_non_rust_and_external_redirects_fail_closed() {
        let repo = TempRepo::new();
        fs::write(
            repo.root.join("apps/example/tests/hidden.rs"),
            "unsafe {}\n",
        )
        .expect("hidden test source is writable");
        fs::write(repo.root.join("apps/example/src/risky.inc"), "unsafe {}\n")
            .expect("non-Rust include is writable");
        fs::create_dir_all(repo.root.join("hidden"))
            .expect("outside source directory is creatable");
        fs::write(repo.root.join("hidden/risky.rs"), "unsafe {}\n")
            .expect("outside source is writable");

        assert!(validate_include_expression(
            &repo.root,
            &repo.source(),
            "::core::concat!(\"../te\", \"sts/hidden.rs\")",
            AuditMode::Current,
        )
        .is_err());
        assert!(validate_include_expression(
            &repo.root,
            &repo.source(),
            "\"risky.inc\"",
            AuditMode::Current,
        )
        .is_err());
        assert!(
            validate_path_redirect(&repo.root, &repo.source(), "../../../hidden/risky.rs").is_err()
        );
    }

    #[test]
    fn cfg_attr_and_macro_emitted_source_redirects_fail_closed() {
        let repo = TempRepo::new();
        fs::write(
            repo.root.join("apps/example/tests/hidden.rs"),
            "unsafe {}\n",
        )
        .expect("hidden test source is writable");
        let cfg_attr = audit_source(
            r#"
                #[cfg_attr(not(test), path = "../tests/hidden.rs")]
                mod hidden;
            "#,
        )
        .expect("cfg_attr fixture parses");
        assert_eq!(cfg_attr.path_redirects, ["../tests/hidden.rs"]);
        assert!(
            validate_path_redirect(&repo.root, &repo.source(), &cfg_attr.path_redirects[0])
                .is_err()
        );

        let macro_redirect = audit_source(
            r#"
                macro_rules! hidden {
                    () => { include!(concat!("../te", "sts/hidden.rs")); };
                }
                macro_rules! attach {
                    ($attribute:ident, $target:literal, $module:ident) => {
                        #[$attribute = $target]
                        mod $module;
                    };
                }
                attach!(path, "../tests/hidden.rs", hidden);
            "#,
        )
        .expect("macro redirect fixture parses");
        assert!(!macro_redirect.invalid_attributes.is_empty());
    }

    #[test]
    fn cargo_production_targets_and_dependencies_cannot_escape_scan_roots() {
        let repo = TempRepo::new();
        fs::write(repo.root.join("apps/example/tests/lib.rs"), "unsafe {}\n")
            .expect("test library is writable");
        fs::create_dir_all(repo.root.join("hidden-risk"))
            .expect("outside package directory is creatable");

        fs::write(
            repo.root.join("apps/example/Cargo.toml"),
            "[package]\nname = \"example\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"tests/lib.rs\"\n",
        )
        .expect("library escape manifest is writable");
        assert!(validate_manifest(&repo.root, &repo.root.join("apps/example/Cargo.toml")).is_err());

        fs::write(
            repo.root.join("apps/example/Cargo.toml"),
            "[package]\nname = \"example\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nrisky = { path = \"../../hidden-risk\" }\n",
        )
        .expect("dependency escape manifest is writable");
        assert!(validate_manifest(&repo.root, &repo.root.join("apps/example/Cargo.toml")).is_err());

        fs::create_dir_all(repo.root.join("hidden-risk-replacement/src"))
            .expect("replacement source directory is creatable");
        fs::write(
            repo.root.join("hidden-risk-replacement/Cargo.toml"),
            "[package]\nname = \"anyhow\"\nversion = \"1.0.103\"\nedition = \"2021\"\n",
        )
        .expect("replacement manifest is writable");
        fs::write(
            repo.root.join("hidden-risk-replacement/src/lib.rs"),
            "pub unsafe fn hidden() {}\n",
        )
        .expect("replacement source is writable");
        fs::write(
            repo.root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"apps/example\"]\nresolver = \"2\"\n\n[replace]\n\"anyhow:1.0.103\" = { path = \"hidden-risk-replacement\" }\n",
        )
        .expect("replacement workspace manifest is writable");
        assert!(validate_manifests(&repo.root).is_err());

        fs::create_dir_all(repo.root.join(".cargo"))
            .expect("fixture Cargo config directory is creatable");
        fs::write(
            repo.root.join(".cargo/config.toml"),
            "paths = [\"hidden-risk-replacement\"]\n",
        )
        .expect("Cargo path override fixture is writable");
        assert!(validate_cargo_config(&repo.root).is_err());

        fs::write(
            repo.root.join(".cargo/config.toml"),
            "[target.aarch64-unknown-none]\nrustflags = [\"--cfg\", \"test\"]\n",
        )
        .expect("Cargo cfg(test) override fixture is writable");
        assert!(validate_cargo_config(&repo.root).is_err());

        fs::write(
            repo.root.join(".cargo/config.toml"),
            "[build]\ntarget = \"aarch64-unknown-none\"\n",
        )
        .expect("Cargo default-target override fixture is writable");
        assert!(validate_cargo_config(&repo.root).is_err());

        fs::write(
            repo.root.join(".cargo/config.toml"),
            "[target.aarch64-apple-darwin]\nrunner = \"true\"\n",
        )
        .expect("Cargo runner override fixture is writable");
        assert!(validate_cargo_config(&repo.root).is_err());

        fs::write(
            repo.root.join(".cargo/config.toml"),
            "[target.aarch64-apple-darwin]\nlinker = \"false\"\n",
        )
        .expect("Cargo linker override fixture is writable");
        assert!(validate_cargo_config(&repo.root).is_err());

        fs::write(repo.root.join(".cargo/config.toml"), "")
            .expect("root Cargo config fixture is resettable");
        fs::create_dir_all(repo.root.join("apps/example/.cargo"))
            .expect("nested Cargo config directory is creatable");
        fs::write(
            repo.root.join("apps/example/.cargo/config.toml"),
            "[build]\nrustflags = [\"--cfg=test\"]\n",
        )
        .expect("nested Cargo config fixture is writable");
        assert!(validate_cargo_config(&repo.root).is_err());

        fs::remove_dir_all(repo.root.join("apps/example/.cargo"))
            .expect("nested app Cargo config fixture is removable");
        fs::create_dir_all(repo.root.join("tests/.Cargo"))
            .expect("tests-only case-variant Cargo config directory is creatable");
        fs::write(
            repo.root.join("tests/.Cargo/Config.Toml"),
            "[build]\nrustflags = [\"--cfg=test\"]\n",
        )
        .expect("tests-only case-variant Cargo config fixture is writable");
        assert!(validate_cargo_config(&repo.root).is_err());
        fs::remove_dir_all(repo.root.join("tests/.Cargo"))
            .expect("tests-only Cargo config fixture is removable");

        fs::write(
            repo.root.join("apps/example/build.rs"),
            "fn main() { println!(\"cargo:rustc-cfg=test\"); }\n",
        )
        .expect("uncontracted build script fixture is writable");
        fs::write(
            repo.root.join("apps/example/Cargo.toml"),
            "[package]\nname = \"example\"\nversion = \"0.1.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n",
        )
        .expect("build-script fixture manifest is writable");
        assert!(validate_manifest(&repo.root, &repo.root.join("apps/example/Cargo.toml")).is_err());

        fs::write(
            repo.root.join("apps/example/Cargo.toml"),
            "[package]\nname = \"example\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlinks = \"risk-escape\"\n",
        )
        .expect("Cargo links override fixture manifest is writable");
        assert!(validate_manifest(&repo.root, &repo.root.join("apps/example/Cargo.toml")).is_err());
    }

    #[test]
    fn nested_package_manifest_cannot_hide_a_production_tests_module() {
        let repo = TempRepo::new();
        let hidden = repo.root.join("apps/example/src/hiddenpkg");
        fs::create_dir_all(hidden.join("tests"))
            .expect("nested fixture tests directory is creatable");
        fs::write(
            hidden.join("Cargo.toml"),
            "[package]\nname = \"hiddenpkg\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("nested fixture manifest is writable");
        fs::write(hidden.join("mod.rs"), "mod tests;\n")
            .expect("nested production module is writable");
        fs::write(
            hidden.join("tests/mod.rs"),
            "fn hidden(value: Option<u8>) { let _ = value.unwrap(); }\n",
        )
        .expect("hidden production source is writable");

        assert!(validate_manifests(&repo.root).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn extensionless_symlink_module_redirect_fails_closed() {
        use std::os::unix::fs::symlink;

        let repo = TempRepo::new();
        fs::write(repo.root.join("apps/example/tests/risky.rs"), "unsafe {}\n")
            .expect("symlink target is writable");
        symlink(
            repo.root.join("apps/example/tests/risky.rs"),
            repo.root.join("apps/example/src/risky"),
        )
        .expect("fixture symlink is creatable");

        assert!(validate_path_redirect(&repo.root, &repo.source(), "risky").is_err());
    }

    #[test]
    fn current_repository_generator_and_include_contracts_are_complete() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest_dir
            .parent()
            .and_then(Path::parent)
            .expect("tool package is nested below repository root");
        let root = fs::canonicalize(root).expect("repository root is canonicalizable");

        let global = count_tree(&root, AuditMode::Current)
            .expect("current repository source and generator contracts pass");
        let linked = count_paths(&root, &LINKED_RUNTIME_HAL_PATHS, AuditMode::Current)
            .expect("current linked-runtime/HAL component contracts pass");
        let outside = global
            .checked_sub(linked)
            .expect("linked component remains within current global counts");
        let baseline = parse_baseline(COMPLETE_BASELINE).expect("current baseline parses");
        enforce_budgets(global, linked, outside, &baseline)
            .expect("current repository counts remain within immutable ceilings");
    }
}
