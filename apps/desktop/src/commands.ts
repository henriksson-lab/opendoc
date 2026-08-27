import type { AppDocument } from "./types";

export type DesktopCommandArgs = {
  create_document: { title: string };
  get_document: {};
  add_paragraph: { text: string };
  add_heading: { text: string; level: number };
  add_link: { text: string; href: string };
  add_mention: { label: string };
  add_footnote_ref: {};
  add_equation: { source: string };
  add_equation_block: { source: string };
  add_list_item: { text: string; level: number; ordered: boolean };
  add_page_break: {};
  add_table: {};
  add_citation: {};
  add_comment: { author: string; body: string };
  add_suggestion: { author: string; text: string };
  update_inline_text: { inlineId: string; text: string };
  delete_comment_thread: { threadId: string };
  accept_suggestion: { suggestionId: string; acceptedBy: string };
  reject_suggestion: { suggestionId: string; rejectedBy: string };
  add_text_mark: { inlineId: string; markKind: string };
  update_block_equation_source: { blockId: string; source: string };
  set_spreadsheet_cell: { address: string; value: string };
  update_bibliography_reference: {
    referenceId: string;
    title: string;
    issued: string | null;
  };
  save_local_repository: { path: string };
  open_local_repository: { path: string; documentUuid: string };
  sign_with_openssh_private_key: {
    privateKeyPem: string;
    signerDisplay: string;
  };
  verify_current_signature: { privateKeyPem: string };
};

export type DesktopCommandName = keyof DesktopCommandArgs;
export type DocumentCommandName = Exclude<DesktopCommandName, "verify_current_signature">;
export type CommandArgs<K extends DesktopCommandName> = DesktopCommandArgs[K];
export type CommandResult<K extends DesktopCommandName> = K extends "verify_current_signature"
  ? string
  : AppDocument;
