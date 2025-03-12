import { StateSnapshot, TaskStatus, WebsocketOpKind } from "../types";
import { getDaysInMonth } from 'date-fns';

// Constants
type MonthKey = 'january' | 'jan' | 'february' | 'feb' | 'march' | 'mar' | 'april' | 'apr' |
                'may' | 'june' | 'jun' | 'july' | 'jul' | 'august' | 'aug' | 'september' |
                'sep' | 'october' | 'oct' | 'november' | 'nov' | 'december' | 'dec';

const MONTH_MAPPINGS: Record<MonthKey, number> = {
  january: 0, jan: 0,
  february: 1, feb: 1,
  march: 2, mar: 2,
  april: 3, apr: 3,
  may: 4,
  june: 5, jun: 5,
  july: 6, jul: 6,
  august: 7, aug: 7,
  september: 8, sep: 8,
  october: 9, oct: 9,
  november: 10, nov: 10,
  december: 11, dec: 11
};

const DAYS_OF_WEEK = ['sunday', 'monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday'];

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

export function parseDueCommand(input: string): number | null {
  const parts = input.trim().split(' ');
  if (parts.length < 2) return null;

  const [command, ...timeStrParts] = parts;
  if (command !== 'd') return null;

  const timeStr = timeStrParts.join(' ').toLowerCase();

  // 1st priority: minutes format (e.g., "30m")
  const minutesMatch = timeStr.match(/^(\d+)m$/);
  if (minutesMatch) {
    const minutes = parseInt(minutesMatch[1], 10);
    if (isNaN(minutes)) return null;
    return Math.floor(Date.now() / 1000) + (minutes * 60);
  }

  // 2nd priority: hours format (e.g., "2h")
  const hoursMatch = timeStr.match(/^(\d+)h$/);
  if (hoursMatch) {
    const hours = parseInt(hoursMatch[1], 10);
    if (isNaN(hours)) return null;
    return Math.floor(Date.now() / 1000) + (hours * 60 * 60);
  }

  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());

  // 3rd priority: absolute times (e.g., "8am", "9pm", "3:30 pm")
  const timeRegex = /^(\d{1,2})(?::(\d{2}))?\s*(am|pm)$/;
  const timeMatch = timeStr.match(timeRegex);
  if (timeMatch) {
    let [_, hours, minutes, meridiem] = timeMatch;
    let parsedHours = parseInt(hours, 10);
    const parsedMinutes = minutes ? parseInt(minutes, 10) : 0;

    // Convert to 24-hour format
    if (meridiem === 'pm' && parsedHours !== 12) parsedHours += 12;
    if (meridiem === 'am' && parsedHours === 12) parsedHours = 0;

    let targetDate = new Date(today);
    targetDate.setHours(parsedHours, parsedMinutes, 0, 0);

    // If the time has already passed today, set it to tomorrow
    if (targetDate <= now) {
      targetDate.setDate(targetDate.getDate() + 1);
    }

    return Math.floor(targetDate.getTime() / 1000);
  }

  // 4th priority: dates (e.g., "Jan 17", "Mar 3", "Dec 9")
  const dateRegex = new RegExp(`^(${Object.keys(MONTH_MAPPINGS).join('|')})\\s+(\\d{1,2})$`);
  const dateMatch = timeStr.match(dateRegex);
  if (dateMatch) {
    const [_, month, dayStr] = dateMatch;
    const monthIndex = MONTH_MAPPINGS[month as MonthKey];
    const day = parseInt(dayStr, 10);
    
    // Validate day of month
    const daysInMonth = getDaysInMonth(new Date(today.getFullYear(), monthIndex));
    if (day < 1 || day > daysInMonth) return null;
    
    let targetDate = new Date(today);
    targetDate.setMonth(monthIndex, day);
    targetDate.setHours(0, 0, 0, 0);

    // If the date has already passed this year, set it to next year
    if (targetDate <= now) {
      targetDate.setFullYear(targetDate.getFullYear() + 1);
      // Revalidate the day for the new year (handles leap years)
      const daysInMonthNextYear = getDaysInMonth(new Date(targetDate.getFullYear(), monthIndex));
      if (day > daysInMonthNextYear) return null;
    }

    return Math.floor(targetDate.getTime() / 1000);
  }

  // 5th priority: dates + times (e.g., "Jan 15 5 pm", "Nov 30 1:05 am")
  const dateTimeRegex = new RegExp(`^(${Object.keys(MONTH_MAPPINGS).join('|')})\\s+(\\d{1,2})\\s+(\\d{1,2})(?::(\\d{2}))?\\s*(am|pm)$`);
  const dateTimeMatch = timeStr.match(dateTimeRegex);
  if (dateTimeMatch) {
    const [_, month, dayStr, hours, minutes, meridiem] = dateTimeMatch;
    const monthIndex = MONTH_MAPPINGS[month as MonthKey];
    const day = parseInt(dayStr, 10);
    
    // Validate day of month
    const daysInMonth = getDaysInMonth(new Date(today.getFullYear(), monthIndex));
    if (day < 1 || day > daysInMonth) return null;
    
    let parsedHours = parseInt(hours, 10);
    const parsedMinutes = minutes ? parseInt(minutes, 10) : 0;

    // Validate hours and minutes
    if (parsedHours < 1 || parsedHours > 12 || parsedMinutes < 0 || parsedMinutes > 59) return null;

    // Convert to 24-hour format
    if (meridiem === 'pm' && parsedHours !== 12) parsedHours += 12;
    if (meridiem === 'am' && parsedHours === 12) parsedHours = 0;

    let targetDate = new Date(today);
    targetDate.setMonth(monthIndex, day);
    targetDate.setHours(parsedHours, parsedMinutes, 0, 0);

    // If the date has already passed this year, set it to next year
    if (targetDate <= now) {
      targetDate.setFullYear(targetDate.getFullYear() + 1);
      // Revalidate the day for the new year (handles leap years)
      const daysInMonthNextYear = getDaysInMonth(new Date(targetDate.getFullYear(), monthIndex));
      if (day > daysInMonthNextYear) return null;
    }

    return Math.floor(targetDate.getTime() / 1000);
  }

  // 6th priority: days of week (e.g., "Monday", "Tuesday")
  const dayIndex = DAYS_OF_WEEK.indexOf(timeStr);
  if (dayIndex !== -1) {
    let targetDate = new Date(today);
    const currentDay = targetDate.getDay();
    let daysToAdd = dayIndex - currentDay;
    if (daysToAdd <= 0) daysToAdd += 7;
    
    targetDate.setDate(targetDate.getDate() + daysToAdd);
    targetDate.setHours(0, 0, 0, 0);
    
    return Math.floor(targetDate.getTime() / 1000);
  }

  // 7th priority: days of week + time (e.g., "Monday 5pm", "Tuesday 8am")
  const dayTimeRegex = new RegExp(`^(${DAYS_OF_WEEK.join('|')})\\s+(\\d{1,2})(?::(\\d{2}))?\\s*(am|pm)$`);
  const dayTimeMatch = timeStr.match(dayTimeRegex);
  if (dayTimeMatch) {
    const [_, day, hours, minutes, meridiem] = dayTimeMatch;
    const dayIndex = DAYS_OF_WEEK.indexOf(day);
    
    let parsedHours = parseInt(hours, 10);
    const parsedMinutes = minutes ? parseInt(minutes, 10) : 0;

    // Convert to 24-hour format
    if (meridiem === 'pm' && parsedHours !== 12) parsedHours += 12;
    if (meridiem === 'am' && parsedHours === 12) parsedHours = 0;

    let targetDate = new Date(today);
    const currentDay = targetDate.getDay();
    let daysToAdd = dayIndex - currentDay;
    if (daysToAdd <= 0) daysToAdd += 7;
    
    targetDate.setDate(targetDate.getDate() + daysToAdd);
    targetDate.setHours(parsedHours, parsedMinutes, 0, 0);
    
    return Math.floor(targetDate.getTime() / 1000);
  }

  return null;
}