import { TodosCache } from "../types";

// Local storage operations (simpler equivalent of xdg_manager from the Rust version)
export const saveCache = (cache: TodosCache): void => {
  localStorage.setItem("todosCache", JSON.stringify(cache));
};

export const loadCache = (): TodosCache | null => {
  const cachedData = localStorage.getItem("todosCache");
  if (!cachedData) return null;
  
  try {
    return JSON.parse(cachedData) as TodosCache;
  } catch (e) {
    console.error("Failed to parse cache:", e);
    return null;
  }
};

export const clearCache = (): void => {
  localStorage.removeItem("todosCache");
};