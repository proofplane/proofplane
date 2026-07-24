# Controls and Mappings

Start with `list_frameworks`, choose the applicable framework, then call `list_framework_requirements` and retain the requirement IDs you need. Requirements are global framework statements; controls are the safeguards that define what must be proven.

Use `create_control` to define a control and link its complete set of framework requirement IDs. Use `replace_control` when its wording or requirement links change; the supplied requirement list replaces the previous list. Read controls with `list_controls` and `get_control`.

Map a piece of evidence to a control with `map_evidence_to_control`. Write a rationale that states how that proof demonstrates the control, rather than merely repeating either title. Inspect mappings with `list_evidence_control_mappings`, and remove a link with `remove_evidence_control_mapping` only when the evidence no longer supports that control.

When you have several links to make or break at once, reach for a batch tool instead of one call per pair. A batch fans out from a single anchor to many counterparts, so each direction is its own tool: `map_evidence_to_controls` maps one evidence to many controls (each with its own rationale), and `map_control_to_evidence` maps one control to many evidence; `unmap_evidence_from_controls` and `unmap_control_from_evidence` are the removal halves. There is no both-sides form — pick the anchor, then list its counterparts.

Every batch is all-or-nothing and capped at 50 items. It applies completely or, if any counterpart is rejected, writes nothing at all and reports which IDs failed and why. Because a rejected batch leaves no partial state, simply fix the offending IDs and resend the corrected batch — do not track which pairs applied or write retry logic that re-maps them individually.
