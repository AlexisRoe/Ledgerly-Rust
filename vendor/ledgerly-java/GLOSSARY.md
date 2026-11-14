# Ledgerly

Ledgerly lets small teams record expenses, attach receipts, and move each expense through an approval workflow.

## Teams and people

**Team**:
A group of users who share expenses and an approval workflow. Every expense belongs to exactly one team.

**Membership**:
A user's belonging to a team, carrying exactly one role.

**Invitation**:
An owner's offer to an email address to join a team with a given role. The invitee accepts or declines it; it expires if neither happens.
_Avoid_: Add member

**Verified**:
A user who has proven they control their email address. Only verified users can accept invitations.

**Teammate**:
Any user who holds a membership in a team, whatever their role.
_Avoid_: Member (when meaning anyone in the team), user (when meaning someone in the team)

**Role**:
A rank within a team. Ranks form a ladder: Owner includes everything an Approver may do, and Approver includes everything a Member may do.
_Avoid_: Permission, group

**Owner**:
The highest role; administers the team, records reimbursements and reads the audit trail. A team always has at least one. Refers only to the team role, never to the person behind an expense.

**Member**:
The lowest role; may claim expenses and sees only their own. Refers only to the role, never to teammates in general.

**Approver**:
A role that may approve or reject other people's submitted expenses.

**Claimant**:
The user who incurred an expense and asks to be paid back for it. A claimant keeps that status on their expenses even after leaving the team.
_Avoid_: Owner, author, creator, submitter

## Expenses

**Expense**:
A single amount spent on behalf of a team, with a date, merchant, category and optional receipts, claimed by one claimant.

**Money**:
An amount together with its currency. Amounts in different currencies are never added together.

**Category**:
One of a fixed, system-wide set of expense kinds (for example Travel, Meals, Office, Software, Other).

**Expense status**:
Where an expense stands in the workflow: Draft, Submitted, Approved, Rejected or Reimbursed.

**Draft**:
An expense still being prepared by its claimant. Only a draft may be edited, have receipts added or removed, or be deleted.

**Submitted**:
An expense handed in for approval and waiting for an approver's decision.

**Approved**:
An expense an approver has accepted for repayment. Cannot be undone; it can only move on to reimbursed.

**Rejected**:
An expense an approver has turned down, always with a written rejection reason. Its claimant can reopen it.

**Rejection reason**:
The explanation an approver gives when rejecting, kept with that rejection so every rejection in an expense's history keeps its own reason.

**Reimbursed**:
An approved expense that an owner has recorded as paid back to the claimant. Ledgerly records the fact; it does not move money. Final.

**Withdraw**:
A claimant pulling their own submitted expense back to a draft before anyone has decided on it.
_Avoid_: Cancel, retract

**Reopen**:
Turning a rejected expense back into a draft so the claimant can fix it and submit it again.
_Avoid_: Resubmit, revert

**Self-dealing**:
Approving, rejecting or reimbursing an expense you claimed yourself. Never allowed, which means a one-person team can never get its expenses approved.
_Avoid_: Self-approval (too narrow)

## Receipts

**Receipt**:
A file (PDF, JPEG or PNG) attached to an expense as evidence of the spending.

**Scan status**:
The result of checking a receipt for malware: Pending scan, Accepted or Quarantined.

**Accepted** (receipt):
A receipt that passed the scan and may be downloaded.

**Quarantined**:
A receipt that failed the scan. It is kept out of reach and never served.

**Possible duplicate**:
A receipt whose file is identical to one already attached to another expense in the same team. A warning for approvers, not a block.

## Imports

**Import**:
A teammate's upload of a CSV file that creates many draft expenses at once, with the uploader as claimant. Rows that fail validation are skipped and reported by line.
_Avoid_: Bulk upload

## Accountability

**Audit trail**:
The permanent, append-only record of who did what in a team, and when. Readable by owners only.
_Avoid_: Log, history
