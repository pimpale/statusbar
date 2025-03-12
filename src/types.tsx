import { z } from "zod";

// Basic types
export const TaskStatusSchema = z.enum(["Succeeded", "Failed", "Obsoleted"]);
export type TaskStatus = z.infer<typeof TaskStatusSchema>;

export const LiveTaskSchema = z.object({
    id: z.string(),
    value: z.string(),
    deadline: z.number().nullable(),
    managed: z.string().nullable()
});
export type LiveTask = z.infer<typeof LiveTaskSchema>;

export const FinishedTaskSchema = LiveTaskSchema.extend({
    status: TaskStatusSchema
});
export type FinishedTask = z.infer<typeof FinishedTaskSchema>;

export const StateSnapshotSchema = z.object({
    live: z.array(LiveTaskSchema),
    finished: z.array(FinishedTaskSchema)
});
export type StateSnapshot = z.infer<typeof StateSnapshotSchema>;

// Cache types
export interface TodosCache {
  serverApiUrl: string;
  apiKey: string;
}

// View type enum
export enum ViewType {
  Live = "live",
  Finished = "finished",
  Overdue = "overdue"
}

// App state types
export type AppState =
  | { type: "NotLoggedIn"; error?: string }
  | { type: "Restored"; apiKey: string }
  | { type: "NotConnected"; apiKey: string; error?: string }
  | {
      type: "Connected";
      apiKey: string;
      inputValue: string;
      activeIdVal?: [id: string, value: string, deadline: number | null];
      snapshot: StateSnapshot;
      viewType: ViewType;
      sessionId: string;
    };

// WebSocket operation kinds
export const WebsocketOpKindSchema = z.object({
    OverwriteState: StateSnapshotSchema.optional(),
    InsLiveTask: z.object({
        id: z.string(),
        value: z.string(),
        deadline: z.number().nullable()
    }).optional(),
    RestoreFinishedTask: z.object({
        id: z.string()
    }).optional(),
    EditLiveTask: z.object({
        id: z.string(),
        value: z.string(),
        deadline: z.number().nullable()
    }).optional(),
    DelLiveTask: z.object({
        id: z.string()
    }).optional(),
    MvLiveTask: z.object({
        id_del: z.string(),
        id_ins: z.string()
    }).optional(),
    RevLiveTask: z.object({
        id1: z.string(),
        id2: z.string()
    }).optional(),
    FinishLiveTask: z.object({
        id: z.string(),
        status: TaskStatusSchema
    }).optional()
}).refine(obj => Object.keys(obj).length === 1, "Exactly one operation must be present");
export type WebsocketOpKind = z.infer<typeof WebsocketOpKindSchema>;

export const WebsocketOpSchema = z.object({
    alleged_time: z.number(),
    kind: WebsocketOpKindSchema
});
export type WebsocketOp = z.infer<typeof WebsocketOpSchema>;


// Server info response schema
export const ServerInfoSchema = z.object({
    service: z.string(),
    versionMajor: z.number(),
    versionMinor: z.number(),
    versionRev: z.number(),
    appPubOrigin: z.string().url(),
    authPubApiHref: z.string().url(),
    authAuthenticatorHref: z.string().url(),
});

export type ServerInfo = z.infer<typeof ServerInfoSchema>;