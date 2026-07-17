# Controls and Mappings

Start with `list_frameworks`, choose the applicable framework, then call `list_framework_requirements` and retain the requirement IDs you need. Requirements are global framework statements; controls are the safeguards that define what must be proven.

Use `create_control` to define a control and link its complete set of framework requirement IDs. Use `replace_control` when its wording or requirement links change; the supplied requirement list replaces the previous list. Read controls with `list_controls` and `get_control`.

Map a piece of evidence to a control with `map_evidence_to_control`. Write a rationale that states how that proof demonstrates the control, rather than merely repeating either title. Inspect mappings with `list_evidence_control_mappings`, and remove a link only when the evidence no longer supports that control.
