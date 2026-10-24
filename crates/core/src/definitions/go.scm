; What names a Go file defines (see definitions.rs for the captures).

(function_declaration name: (identifier) @def)
(type_spec name: (type_identifier) @def)
(type_alias name: (type_identifier) @def)
(var_spec name: (identifier) @def)
(const_spec name: (identifier) @def)
(parameter_declaration name: (identifier) @def)
(variadic_parameter_declaration name: (identifier) @def)
(type_parameter_declaration name: (identifier) @def)
(short_var_declaration left: (expression_list) @pattern)
(range_clause left: (expression_list) @pattern)
(type_switch_statement alias: (expression_list) @pattern)

(method_declaration name: (field_identifier) @member)
(field_declaration name: (field_identifier) @member)
(method_elem name: (field_identifier) @member)

(import_spec name: (package_identifier) @import)
(import_spec name: (dot) @glob)

[
  (function_declaration)
  (method_declaration)
  (func_literal)
  (block)
  (for_statement)
  (if_statement)
  (expression_switch_statement)
  (type_switch_statement)
] @scope
