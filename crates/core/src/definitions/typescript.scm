; What names a TypeScript or TSX file defines (see definitions.rs for the captures).

(function_declaration name: (identifier) @def)
(generator_function_declaration name: (identifier) @def)
(function_signature name: (identifier) @def)
(class_declaration name: (type_identifier) @def)
(abstract_class_declaration name: (type_identifier) @def)
(interface_declaration name: (type_identifier) @def)
(type_alias_declaration name: (type_identifier) @def)
(enum_declaration name: (identifier) @def)
(internal_module name: (identifier) @def)
(type_parameter name: (type_identifier) @def)
(variable_declarator name: (_) @pattern)
(required_parameter pattern: (_) @pattern)
(optional_parameter pattern: (_) @pattern)
(arrow_function parameter: (identifier) @def)
(catch_clause parameter: (_) @pattern)
(for_in_statement left: (_) @pattern)

(method_definition name: [(property_identifier) (private_property_identifier)] @member)
(abstract_method_signature name: (property_identifier) @member)
(public_field_definition name: [(property_identifier) (private_property_identifier)] @member)
(property_signature name: (property_identifier) @member)
(method_signature name: (property_identifier) @member)
(enum_body name: (property_identifier) @member)
(enum_assignment name: (property_identifier) @member)
(pair key: (property_identifier) @member)
(required_parameter (accessibility_modifier) pattern: (identifier) @member)
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
(import_require_clause (identifier) @import)

[
  (statement_block)
  (function_declaration)
  (generator_function_declaration)
  (function_signature)
  (function_expression)
  (arrow_function)
  (method_definition)
  (for_statement)
  (for_in_statement)
  (catch_clause)
  (class_declaration)
  (abstract_class_declaration)
  (interface_declaration)
  (type_alias_declaration)
] @scope
