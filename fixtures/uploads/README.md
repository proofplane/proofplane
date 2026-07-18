# Upload Fixtures

Sample files for the human browser upload flow at `/evidence-uploads`. Get a
link by calling the MCP tool `manage_evidence_submissions` with an evidence ID
and the coverage window the proof covers, then open the returned URL and choose
one of these files.

- `vuln-scanner-results.txt` — a clean file. Its submission should reach
  `upload_status` `uploaded` once the dequeuer and worker finish.
- `eicar.com.txt` — the standard EICAR anti-malware test signature. It is
  harmless text, not executable malware, but ClamAV and other antivirus
  products intentionally detect it as malicious, so its submission should land
  on `contains_virus` and refuse to download. Host antivirus software may
  quarantine this file.

Confirm either outcome with `list_evidence_submissions`.
