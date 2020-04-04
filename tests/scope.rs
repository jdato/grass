#![cfg(test)]

#[macro_use]
mod macros;

test!(
    scoping_var_decl_inner_ruleset,
    "a {\n  $color: red;\n  b {\n    $color: blue;\n  }\n  color: $color;\n}\n",
    "a {\n  color: blue;\n}\n"
);
test!(
    basic_global,
    "a {\n  $color: red !global;\n}\n\nb {\n  color: $color;\n}\n",
    "b {\n  color: red;\n}\n"
);
test!(
    global_inserted_into_local_and_global_scopes,
    "$foo: 42;\n\n.foo {\n  content: $foo;\n  $foo: 1337 !global;\n  content: $foo;\n}\n\n.bar {\n  content: $foo;\n}\n",
    ".foo {\n  content: 42;\n  content: 1337;\n}\n\n.bar {\n  content: 1337;\n}\n"
);
test!(
    global_in_mixin,
    "$y: a;\n@mixin foo {\n  $y: b !global;\n}\na {\n  @include foo;\n  color: $y;\n}\n",
    "a {\n  color: b;\n}\n"
);
