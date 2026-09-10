# Issue intake — 2026-09-10

Raw operator intake captured by Ryan Merlin against the `integration/phase-1`
release binary. **Not yet transcribed into `operator-followups.md`** — that is
the next session's first task. Kept verbatim so nothing is lost in summary.

One correction when reading: the `job-groups profile` paste below shows
`error_code: validation`, which is the pre-`680d308` binary. On this branch it
reports `not_found`.

---

## One Connections
- ayx one connections list
Needs to show the owner at least, Owner ID and Owner Name (ensure populated)
Needs to show the connection type, as in what it's connected to; bigquery, snowflake, GCS, ... etc


- How hard is it to remove the completely useless microseconds from displaying.  It seems its not being used by the alteryx software, and just adds more visual clutter.

PS C:\code\worktrees\ayx-rs\integration-phase-1> ayx one connections list
connection list ok (11 items, 1 page)
ID     NAME                            CREATEDAT                 UPDATEDAT
-----  ------------------------------  ------------------------  ------------------------
44865  land-lease-intel-bq             2026-07-20T22:53:01.000Z  2026-08-26T16:49:21.000Z
44847  land-lease-intel                2026-07-20T21:05:25.000Z  2026-07-20T22:40:39.000Z
44845  NM-Wells-Public                 2026-07-20T20:39:06.000Z  2026-08-05T21:05:35.000Z
43438  fde3_gcp_conn                   2026-07-15T13:57:39.000Z  2026-07-15T13:57:39.000Z
42550  fde3_chatgpt_apikey             2026-06-24T16:33:19.000Z  2026-06-24T20:30:14.000Z
42137  partner-business-dev-7572       2026-06-15T21:03:19.000Z  2026-06-15T21:03:19.000Z
42043  merlin-sheets1                  2026-06-12T18:09:52.000Z  2026-06-12T18:09:52.000Z
40995  SNOWFLAKE_PBIA_EXTRACT          2026-05-26T17:49:21.000Z  2026-05-26T21:20:21.000Z
39339  PJ bigquery                     2026-04-22T17:44:34.000Z  2026-04-22T17:44:34.000Z
38273  google_bigquery_fde_connection  2026-03-26T14:37:14.000Z  2026-07-07T15:14:16.000Z
38176  bigquery_fde_connection         2026-03-25T14:39:11.000Z  2026-03-25T18:01:35.000Z





## ayx one connections detail
Again, this needs to have the important information displayed, and currently does not.
This should show: connection source type, who owns it, count of how many it's shared with, and preferrably the last time it was accessed/run.

PS C:\code\worktrees\ayx-rs\integration-phase-1> ayx one connections detail 44865
connection detail ok
  createdAt: 2026-07-20T22:53:01.000Z
  description: project bq SA
  id: 44865
  name: land-lease-intel-bq
  updatedAt: 2026-08-26T16:49:21.000Z



## ayx one connections permissions list
  This needs more info on it and it's broken.   right now it only shows ID col being populated, the NAME col is blank.    And also how is a duplicate ID in the list?
  The missing information is, again, what is the governance info needed, so needs when permission granted, last time it was accessed, how many workflows it's attached to, which person is the owner of the connection.


PS C:\code\worktrees\ayx-rs\integration-phase-1> ayx one connections permissions list 44865
connection permissions ok
ID      NAME
------  ----
646
646
34
113168



## ayx one connections permissions detail
Again, this is meaningless right now, NAME is broken, ID gives nothing meaninful.   
Is this all that is returned, or is this deliberately filtering out all the relavent info?


PS C:\code\worktrees\ayx-rs\integration-phase-1> ayx one connections permissions detail 44865 113168
connection permissions-detail ok (subject 113168)
  id: 113168
  name:



## onboard
- this needs to be cleaned up, and enhanced to prompt the user for the API Token path, not just describe the command.  The fucking point of the wizard is to walk the user through in an easy an understanable way.
- the wizard NEEDS to ask the user which auth type to use, default to OTP, give the API token option.  
- All steps needs a well formatted, easy to read, colorized for key terms, links, Id values, etc
- What do you need to ensure you understand what I mean by visually clean and human readable?
- My thought here is like openclaw, it has multiple script/routines that walk the user through making selections, choosing values, and it runs checks; such as openclaw doctor.


PS C:\code\worktrees\ayx-rs\integration-phase-1> .\target\release\ayx.exe onboard
AYX onboarding
Press Enter to accept a default. Existing values are reused unless you choose to change them.
Profile name [local]: otp-test
Your Alteryx One account email. Leave blank if you only use Alteryx Server.
Email address: ryan.merlin@alteryx.com
Paste your Alteryx One workspace URL (from your browser's address bar),
e.g. https://us1.alteryxcloud.com/auth-portal/workspaces/01ABC…  — or just the
workspace id. Leave blank to skip (you can set it later at login).
Workspace URL or id: https://us1.alteryxcloud.com/ayx-one?workspaceGid=01KMGF85WTTEJZ397MW1RBD9ZB
Configure Alteryx Server [y/N]:

Profile 'otp-test' saved and set as active.

Ready to connect. A one-time passcode will be emailed to ryan.merlin@alteryx.com,
and you'll be asked for your workspace password.
This is a time-limited login: the token lasts 30 days, does not renew
automatically, and you'll sign in again when it runs out.
Prefer not to? `ayx one login --oauth-api-token` is the durable path — paste a
Client ID and Refresh Token once from the Alteryx One UI and access tokens renew
silently from then on. It suits people as well as CI and agents.
Credentials are kept in the operating-system secure store by default (this protects
them at rest; it does not extend how long they last).
Log in now [Y/n]:



- * Another Example:
formatting here, I would think we want: commands highlighted in a consistent color like cobalt blue.  in the key: value sections, can't we highlight the key text to a pallette tuned color (i don't know but perhaps gold)
* And can't we render JSON in the pretty rendering for readability

Profile 'otp-test' saved and set as active.

Ready to connect. A one-time passcode will be emailed to ryan.merlin@alteryx.com,
and you'll be asked for your workspace password.
This is a time-limited login: the token lasts 30 days, does not renew
automatically, and you'll sign in again when it runs out.
Prefer not to? `ayx one login --oauth-api-token` is the durable path — paste a
Client ID and Refresh Token once from the Alteryx One UI and access tokens renew
silently from then on. It suits people as well as CI and agents.
Credentials are kept in the operating-system secure store by default (this protects
them at rest; it does not extend how long they last).
Log in now [Y/n]:
Sending one-time passcode to ryan.merlin@alteryx.com...
(Check your inbox for a 6-digit code)
Enter the 6-digit passcode: 650198
Workspace password:
Save this workspace password securely for future logins? [Y/n] y

Authentication Successful!

Token expires: 1791640201
Credentials stored in profile 'otp-test'.

Connected. Verify any time with:
  ayx one auth status
  ayx one workspace current

This token lasts 30 days and won't renew itself — run `ayx one login` again
when it runs out, or switch to the durable credential once with
`ayx one login --oauth-api-token`.
onboarding completed
  inline_secret_fields:
  login: {"offered":true,"ok":true,"ran":true}
  mode: interactive
  profile: C:\code\ayx-otp-v24\profiles\otp-test.yaml
  saved: true
  secret_refs:
  summary: {"mongo":{"databases":{"gallery_name":"AlteryxGallery","service_name":"AlteryxService"},"mode":"embedded"},"profile_name":"otp-test","server":null,"sqlserver":null}
  validation: {"missing":[],"ok":true}
  warnings:
PS C:\code\worktrees\ayx-rs\integration-phase-1>



## ayx one job-groups list/detail
- In the previous round we flagged 'job-groups' and i said i wanted it to be simplified to 'jobs'  - whats the status of that
- NAME col; not working, all show 'job-?'
- missing info;  needs to show who ran it, how long it took to execute, if any errors, possibly even the workspace since that's relevant, what triggered the run (manaul, schedule, or others), if there are output objects/data it should indicate and identify those refs

PS C:\code\worktrees\ayx-rs\integration-phase-1> ayx one job-groups list
jobGroup list ok (25 items, 1 page)
ID       NAME   STATUS    CREATEDAT                 UPDATEDAT
-------  -----  --------  ------------------------  ------------------------
4087561  job-?  Complete  2026-07-07T23:25:27.000Z  2026-07-07T23:25:34.000Z
4087560  job-?  Complete  2026-07-07T23:25:27.000Z  2026-07-07T23:25:32.000Z
4087559  job-?  Complete  2026-07-07T23:23:36.000Z  2026-07-07T23:23:40.000Z
4087558  job-?  Complete  2026-07-07T23:23:35.000Z  2026-07-07T23:23:40.000Z
4087554  job-?  Complete  2026-07-07T23:23:07.000Z  2026-07-07T23:23:11.000Z



PS C:\code\worktrees\ayx-rs\integration-phase-1> ayx one job-groups detail 4087544
jobGroup detail ok
  createdAt: 2026-07-07T23:21:44.000Z
  description: -
  id: 4087544
  name: -
  status: Complete
  updatedAt: 2026-07-07T23:21:48.000Z


## job-groups status
Doesn't return anything???

PS C:\code\worktrees\ayx-rs\integration-phase-1> ayx one job-groups status 4087544
jobGroup status ok

PS C:\code\worktrees\ayx-rs\integration-phase-1>


## job-groups jobs
looks like this is what shows the job run itself.   
Also missing the metadata listed above, who ran, did it error, outputs, calculated runtime


## job-groups profile
I don't understand what this provides or if it's even working....

PS C:\code\worktrees\ayx-rs\integration-phase-1> ayx one job-groups profile 4087544
jobGroup profile failed
  attempts: 1
  dry_run: false
  elapsed_ms: 472
  endpoint_template: /v4/jobGroups/{id}/profile
  error_code: validation
  method: GET
  mutating: false
  ok: false
  operation: profile
  request_id: 06c38386272d4a78a3aaae20f9f846d1
  response: {"exception":{"details":"Job group 4087544 does not have profiling data","message":"Profiling data not found","name":"ProfilingDataNotFoundException"}}
  response_shape: object
  retry_after_seconds: -
  status_code: 400
  surface: jobGroup
  url: https://us1.alteryxcloud.com/v4/jobGroups/4087544/profile
  validation_target: https://us1.alteryxcloud.com/v4/jobGroups/4087544/profile
PS C:\code\worktrees\ayx-rs\integration-phase-1>


## job-group output
not working!!

PS C:\code\worktrees\ayx-rs\integration-phase-1> ayx one job-groups outputs 4087544
jobGroup outputs ok
The service returned a collection shape this CLI version does not recognize. Use --output json-full to inspect it.
PS C:\code\worktrees\ayx-rs\integration-phase-1>



## for job-groups
is there a better way to structure this command group, hierarchy, simplification, or consolidation?


## job-groups publications
Is there a better way to do this where it flips, to pull all publications and then show the associated job-groups, vs having to drill down individually?

PS C:\code\worktrees\ayx-rs\integration-phase-1> ayx one job-groups publications 4087544
jobGroup publications ok
(no items)


## ayx one agent-assets
- is there a reason for the name 'agent-assets' instead of the product name 'agent-studio' ?


## ayx one agent-assets agents list
Broken:

PS C:\code\worktrees\ayx-rs\integration-phase-1> ayx one agent-assets agents list
command failed
  error: alteryx_one.oauth_client_id is required to refresh access tokens
  error_code: validation
  hint: Inspect the failed flag or input; '--help' on the subcommand documents accepted values.