; What names a Python file defines (see definitions.rs for the captures).

(function_definition name: (identifier) @def)
(class_definition name: (identifier) @def)
(assignment left: (_) @pattern)
(for_statement left: (_) @pattern)
(for_in_clause left: (_) @pattern)
(as_pattern alias: (as_pattern_target) @pattern)
(named_expression name: (identifier) @def)
(parameters (identifier) @def)
(lambda_parameters (identifier) @def)
(default_parameter name: (identifier) @def)
(typed_parameter (identifier) @def)
(typed_default_parameter name: (identifier) @def)
(list_splat_pattern (identifier) @def)
(dictionary_splat_pattern (identifier) @def)

(function_definition
  name: (identifier) @_init
  body: (block
    (expression_statement
      (assignment left: (attribute object: (identifier) @_self attribute: (identifier) @member))))
  (#eq? @_init "__init__")
  (#eq? @_self "self"))

(import_from_statement name: (dotted_name (identifier) @import .))
(import_statement name: (dotted_name . (identifier) @import))
(aliased_import alias: (identifier) @import)
(wildcard_import) @glob

[
  (function_definition)
  (lambda)
  (list_comprehension)
  (set_comprehension)
  (dictionary_comprehension)
  (generator_expression)
] @scope
