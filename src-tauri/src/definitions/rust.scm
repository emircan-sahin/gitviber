; What names a Rust file defines (see definitions.rs for the captures).

(function_item name: (identifier) @def)
(function_signature_item name: (identifier) @def)
(struct_item name: (type_identifier) @def)
(enum_item name: (type_identifier) @def)
(union_item name: (type_identifier) @def)
(trait_item name: (type_identifier) @def)
(type_item name: (type_identifier) @def)
(associated_type name: (type_identifier) @def)
(const_item name: (identifier) @def)
(static_item name: (identifier) @def)
(mod_item name: (identifier) @def body: (_))
(macro_definition name: (identifier) @def)
(type_parameter name: (type_identifier) @def)

(enum_variant name: (identifier) @member)
(field_declaration name: (field_identifier) @member)
; Named through a type or a value (`Self::new`, `x.len()`), never alone.
(impl_item body: (declaration_list [
  (function_item name: (_) @member)
  (const_item name: (_) @member)
  (type_item name: (_) @member)
]))
(trait_item body: (declaration_list [
  (function_item name: (_) @member)
  (function_signature_item name: (_) @member)
  (const_item name: (_) @member)
  (associated_type name: (_) @member)
]))

(parameter pattern: (_) @pattern)
(closure_parameters (_) @pattern)
(let_declaration pattern: (_) @pattern)
(let_condition pattern: (_) @pattern)
(for_expression pattern: (_) @pattern)
(match_arm pattern: (_) @pattern)

; `mod git;` names a file, as an import does.
(mod_item name: (identifier) @import !body)
(use_declaration argument: (identifier) @import)
(use_declaration argument: (scoped_identifier name: (identifier) @import))
(use_as_clause alias: (identifier) @import)
(use_list (identifier) @import)
(use_list (scoped_identifier name: (identifier) @import))
(use_wildcard) @glob

[
  (function_item)
  (closure_expression)
  (block)
  (match_arm)
  (for_expression)
  (if_expression)
  (while_expression)
  (impl_item)
  (trait_item)
  (struct_item)
  (enum_item)
] @scope
