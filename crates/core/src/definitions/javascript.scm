; What names a JavaScript file defines (see definitions.rs for the captures).

(function_declaration name: (identifier) @def)
(generator_function_declaration name: (identifier) @def)
(class_declaration name: (identifier) @def)
(variable_declarator name: (_) @pattern)
(formal_parameters (_) @pattern)
(arrow_function parameter: (identifier) @def)
(catch_clause parameter: (_) @pattern)
(for_in_statement left: (_) @pattern)

(method_definition name: [(property_identifier) (private_property_identifier)] @member)
(field_definition property: [(property_identifier) (private_property_identifier)] @member)
(pair key: (property_identifier) @member)
(method_definition
  name: (property_identifier) @_constructor
  body: (statement_block
    (expression_statement
      (assignment_expression left: (member_expression object: (this) property: (property_identifier) @member))))
  (#eq? @_constructor "constructor"))

(import_specifier name: (identifier) @import !alias)
(import_specifier alias: (identifier) @import)
(import_clause (identifier) @import)
(namespace_import (identifier) @import)

[
  (statement_block)
  (function_declaration)
  (generator_function_declaration)
  (function_expression)
  (arrow_function)
  (method_definition)
  (for_statement)
  (for_in_statement)
  (catch_clause)
] @scope
