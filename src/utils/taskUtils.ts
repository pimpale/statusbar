import { StateSnapshot, TaskStatus, WebsocketOpKind } from "../types";

// Utility functions to match the old Rust implementation
export const currentTimeMillis = (): number => {
  return new Date().getTime();
};

export const randomString = (): string => {
  return Math.random().toString(36).substring(2, 18);
};

// Apply a WebSocket operation to the state snapshot
export const applyOperation = (snapshot: StateSnapshot, op: WebsocketOpKind): StateSnapshot => {
  const newSnapshot = { ...snapshot };

  if (op.OverwriteState) {
    return op.OverwriteState;
  }

  if (op.InsLiveTask) {
    newSnapshot.live = [
      { 
        id: op.InsLiveTask.id, 
        value: op.InsLiveTask.value,
        deadline: op.InsLiveTask.deadline,
        managed: null
      },
      ...newSnapshot.live
    ];
    return newSnapshot;
  }

  if (op.RestoreFinishedTask) {
    const id = op.RestoreFinishedTask.id;
    const finishedIndex = newSnapshot.finished.findIndex(task => task.id === id);
    if (finishedIndex === -1) return snapshot;

    const task = newSnapshot.finished[finishedIndex];
    // remove the task from finished
    newSnapshot.finished = newSnapshot.finished.filter(t => t.id !== id);
    // add the task to live
    newSnapshot.live = [
      { 
        id: task.id, 
        value: task.value,
        deadline: task.deadline,
        managed: task.managed
      },
      ...newSnapshot.live
    ];
    return newSnapshot;
  }

  if (op.EditLiveTask !== undefined) {
    const elt = op.EditLiveTask;
    newSnapshot.live = newSnapshot.live.map(task =>
      task.id === elt.id ? { ...task, value: elt.value, deadline: elt.deadline } : task
    );
    return newSnapshot;
  }

  if (op.DelLiveTask !== undefined) {
    newSnapshot.live = newSnapshot.live.filter(task => task.id !== op.DelLiveTask!.id);
    return newSnapshot;
  }

  if (op.MvLiveTask) {
    const fromIndex = newSnapshot.live.findIndex(task => task.id === op.MvLiveTask!.id_del);
    const toIndex = newSnapshot.live.findIndex(task => task.id === op.MvLiveTask!.id_ins);

    if (fromIndex === -1 || toIndex === -1 || fromIndex === toIndex) {
      return snapshot;
    }

    const task = newSnapshot.live[fromIndex];
    const newList = [...newSnapshot.live];
    newList.splice(fromIndex, 1);
    newList.splice(toIndex, 0, task);

    newSnapshot.live = newList;
    return newSnapshot;
  }

  if (op.RevLiveTask) {
    const index1 = newSnapshot.live.findIndex(task => task.id === op.RevLiveTask!.id1);
    const index2 = newSnapshot.live.findIndex(task => task.id === op.RevLiveTask!.id2);

    if (index1 === -1 || index2 === -1) {
      return snapshot;
    }

    const startIndex = Math.min(index1, index2);
    const endIndex = Math.max(index1, index2);

    const newList = [...newSnapshot.live];
    const section = newList.slice(startIndex, endIndex + 1).reverse();
    newList.splice(startIndex, section.length, ...section);

    newSnapshot.live = newList;
    return newSnapshot;
  }

  if (op.FinishLiveTask) {
    const taskIndex = newSnapshot.live.findIndex(task => task.id === op.FinishLiveTask!.id);
    if (taskIndex === -1) return snapshot;

    const task = newSnapshot.live[taskIndex];

    newSnapshot.live = newSnapshot.live.filter(t => t.id !== op.FinishLiveTask!.id);
    newSnapshot.finished = [
      {
        id: task.id, 
        value: task.value, 
        status: op.FinishLiveTask!.status,
        deadline: task.deadline,
        managed: task.managed
      },
      ...newSnapshot.finished
    ];

    return newSnapshot;
  }

  return snapshot;
};

// Command pattern matching helpers
export const parseRestoreCommand = (input: string): number | null => {
  const match = input.match(/^r\s*(\d+)?$/);
  if (match) {
    return match[1] ? parseInt(match[1]) : 0;
  }
  return null;
};

export const parseMoveToEndCommand = (input: string): number | null => {
  const match = input.match(/^q\s*(\d+)?$/);
  if (match) {
    return match[1] ? parseInt(match[1]) : 0;
  }
  return null;
};

export const parseMoveCommand = (input: string): [number, number] | null => {
  const match = input.match(/^mv\s+(\d+)(?:\s+(\d+))?$/);
  if (match) {
    const fromIndex = parseInt(match[1]);
    const toIndex = match[2] ? parseInt(match[2]) : 0;
    return [fromIndex, toIndex];
  }
  return null;
};

export const parseReverseCommand = (input: string): [number, number] | null => {
  const match = input.match(/^rev\s+(\d+)(?:\s+(\d+))?$/);
  if (match) {
    const fromIndex = parseInt(match[1]);
    const toIndex = match[2] ? parseInt(match[2]) : 0;
    return [fromIndex, toIndex];
  }
  return null;
};