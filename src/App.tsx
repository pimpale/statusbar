import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import "./App.css";
import { StateSnapshot, TaskStatus, WebsocketOp, WebsocketOpKind, WebsocketOpSchema, ServerInfoSchema, ServerInfo } from "./types";


// Extend Window interface to store WebSocket
declare global {
  interface Window {
    todoWebSocket?: WebSocket;
  }
}

type TodosCache = {
  serverApiUrl: string,
  apiKey: string
};

// App state types
type AppState =
  | { type: "NotLoggedIn", email: string, password: string, viewPassword: boolean, error?: string }
  | { type: "Restored", apiKey: string }
  | { type: "NotConnected", apiKey: string, error?: string }
  | {
    type: "Connected",
    apiKey: string,
    inputValue: string,
    activeIdVal?: [string, string],
    snapshot: StateSnapshot,
    showFinished: boolean,
    sessionId: string,
    heartbeatId: string
  };

// Utility functions to match the old Rust implementation
const currentTimeMillis = (): number => {
  return new Date().getTime();
};

const randomString = (): string => {
  return Math.random().toString(36).substring(2, 18);
};

// Apply a WebSocket operation to the state snapshot
const applyOperation = (snapshot: StateSnapshot, op: WebsocketOpKind): StateSnapshot => {
  const newSnapshot = { ...snapshot };

  if (op.OverwriteState) {
    return op.OverwriteState;
  }

  if (op.InsLiveTask) {
    newSnapshot.live = [
      { id: op.InsLiveTask.id, value: op.InsLiveTask.value },
      ...newSnapshot.live
    ];
    return newSnapshot;
  }

  if (op.RestoreFinishedTask) {
    const finishedIndex = newSnapshot.finished.findIndex(task => task.id === op.RestoreFinishedTask!.id);
    if (finishedIndex === -1) return snapshot;

    const task = newSnapshot.finished[finishedIndex];
    newSnapshot.finished = newSnapshot.finished.filter(t => t.id !== op.RestoreFinishedTask!.id);
    newSnapshot.live = [
      { id: task.id, value: task.value },
      ...newSnapshot.live
    ];
    return newSnapshot;
  }

  if (op.EditLiveTask) {
    newSnapshot.live = newSnapshot.live.map(task =>
      task.id === op.EditLiveTask!.id ? { ...task, value: op.EditLiveTask!.value } : task
    );
    return newSnapshot;
  }

  if (op.DelLiveTask) {
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
      { id: task.id, value: task.value, status: op.FinishLiveTask!.status },
      ...newSnapshot.finished
    ];

    return newSnapshot;
  }

  return snapshot;
};

function App() {
  // Main app state
  const [state, setState] = useState<AppState>({
    type: "NotLoggedIn",
    email: "",
    password: "",
    viewPassword: false
  });

  // UI state
  const [expanded, setExpanded] = useState(false);
  const [focused, setFocused] = useState(false);

  // Default server URL
  const defaultServerUrl = "http://localhost:8080/public/";
  const [serverApiUrl, setServerApiUrl] = useState(defaultServerUrl);

  // Refs for inputs
  const emailInputRef = useRef<HTMLInputElement>(null);
  const passwordInputRef = useRef<HTMLInputElement>(null);
  const taskInputRef = useRef<HTMLInputElement>(null);
  const activeTaskInputRef = useRef<HTMLInputElement>(null);

  // Load cached data on mount
  useEffect(() => {
    const cachedData = localStorage.getItem("todosCache");
    if (cachedData) {
      try {
        const cache: TodosCache = JSON.parse(cachedData);
        setServerApiUrl(cache.serverApiUrl);
        setState({ type: "Restored", apiKey: cache.apiKey });
      } catch (e) {
        // Cache invalid, start fresh
        setState({ type: "NotLoggedIn", email: "", password: "", viewPassword: false });
      }
    }
  }, []);

  // Handle expanding the dock
  const expandDock = () => {
    setExpanded(true);

    // Focus the appropriate input
    setTimeout(() => {
      if (state.type === "NotLoggedIn") {
        emailInputRef.current?.focus();
      } else if (state.type === "Connected") {
        taskInputRef.current?.focus();
      }
    }, 0);
  };

  // Handle collapsing the dock
  const collapseDock = () => {
    setExpanded(false);

    // Clear input and active task when collapsed
    if (state.type === "Connected") {
      setState({
        ...state,
        inputValue: "",
        activeIdVal: undefined
      });
    }
  };

  // Handle login attempt
  const attemptLogin = async () => {
    if (state.type !== "NotLoggedIn") return;

    try {
      // Get server info first
      const infoResponse = await fetch(`${serverApiUrl}info`);

      if (!infoResponse.ok) {
        throw new Error(`${infoResponse.status}: ${await infoResponse.text()}`);
      }
  
      // Parse and validate the info response
      const info = ServerInfoSchema.parse(await infoResponse.json());
      const authPubApiUrl = info.authPubApiHref;

      // Get API key
      const loginResponse = await fetch(`${authPubApiUrl}api_key/new_with_email`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json'
        },
        body: JSON.stringify({
          email: state.email,
          password: state.password,
          // 7 days in milliseconds
          duration: 7 * 24 * 60 * 60 * 1000
        })
      });

      if (!loginResponse.ok) {
        throw new Error(`${loginResponse.status}: ${await loginResponse.text()}`);
      }

      const apiKeyData = await loginResponse.json();

      if (!apiKeyData.key) {
        throw new Error("No API key returned");
      }

      // Save to local storage
      const cache: TodosCache = {
        serverApiUrl,
        apiKey: apiKeyData.key
      };
      localStorage.setItem("todosCache", JSON.stringify(cache));

      // Connect to websocket
      connectWebsocket(apiKeyData.key);
    } catch (error) {
      setState({
        ...state,
        error: error instanceof Error ? error.message : "Unknown error"
      });
    }
  };

  // Handle websocket connection
  const connectWebsocket = async (apiKey: string) => {
    // Set state to connecting
    setState({ type: "NotConnected", apiKey });

    try {
      // Create WebSocket URL
      let wsUrl = new URL(serverApiUrl);

      // Convert http(s) to ws(s)
      if (wsUrl.protocol === 'https:') {
        wsUrl.protocol = 'wss:';
      } else {
        wsUrl.protocol = 'ws:';
      }

      wsUrl.pathname += 'ws/task_updates';
      wsUrl.search = `?api_key=${apiKey}`;

      // Create WebSocket
      const ws = new WebSocket(wsUrl.toString());

      // Set up session tracking
      const sessionId = randomString();
      let heartbeatId = randomString();

      // Handle connection open
      ws.addEventListener('open', () => {
        console.log('WebSocket connection established');

        setState({
          type: "Connected",
          apiKey,
          inputValue: "",
          snapshot: {
            live: [],
            finished: []
          },
          showFinished: false,
          sessionId,
          heartbeatId
        });

        // Focus the input
        setTimeout(() => {
          taskInputRef.current?.focus();
        }, 0);
      });

      // In the WebSocket message handler, replace the try/catch block with:
      ws.addEventListener('message', (event) => {
        try {
          // Parse and validate the incoming message
          const wsOp = WebsocketOpSchema.parse(JSON.parse(event.data));
          console.log('Received WebSocket operation:', wsOp.kind);

          setState(prevState => {
            if (prevState.type !== 'Connected') return prevState;

            // Apply the operation to our snapshot
            const newSnapshot = applyOperation(prevState.snapshot, wsOp.kind);

            return {
              ...prevState,
              snapshot: newSnapshot,
              heartbeatId: randomString()
            };
          });
        } catch (error) {
          if (error instanceof z.ZodError) {
            console.error('WebSocket message validation error:', error.errors);
            console.error('Original message:', JSON.parse(event.data));
          } else {
            console.error('WebSocket message error:', error);
          }
        }
      });

      // Handle ping (keep connection alive)
      ws.addEventListener('ping', () => {
        // Reset heartbeat on ping
        setState(prevState => {
          if (prevState.type !== 'Connected') return prevState;
          return {
            ...prevState,
            heartbeatId: randomString()
          };
        });
      });

      // Handle errors
      ws.addEventListener('error', (error) => {
        console.error('WebSocket error:', error);
        setState({
          type: "NotConnected",
          apiKey,
          error: "WebSocket error occurred"
        });
      });

      // Handle close
      ws.addEventListener('close', (event) => {
        console.log('WebSocket closed:', event.code, event.reason);

        if (event.reason === 'Unauthorized') {
          // If unauthorized, go back to login
          setState({
            type: "NotLoggedIn",
            email: "",
            password: "",
            viewPassword: false,
            error: "Session expired. Please log in again."
          });
        } else {
          setState({
            type: "NotConnected",
            apiKey,
            error: event.reason || "Connection closed"
          });
        }
      });

      // Set up timeout checking
      const TIMEOUT_DURATION = 30000; // 30 seconds

      const checkHeartbeat = () => {
        setState(prevState => {
          if (prevState.type !== 'Connected') return prevState;

          // If this is still the same session and heartbeat hasn't changed
          if (prevState.sessionId === sessionId && prevState.heartbeatId === heartbeatId) {
            // Connection timed out
            ws.close();
            return {
              type: "NotConnected",
              apiKey,
              error: "WebSocket connection timed out"
            };
          }

          // Update the heartbeat ID we're checking against
          heartbeatId = prevState.heartbeatId;

          return prevState;
        });

        // Always schedule next check regardless of state
        setTimeout(checkHeartbeat, TIMEOUT_DURATION);
      };

      // Start the timeout checker
      setTimeout(checkHeartbeat, TIMEOUT_DURATION);

      // Store WebSocket for sending messages later
      window.todoWebSocket = ws;

    } catch (error) {
      setState({
        type: "NotConnected",
        apiKey,
        error: error instanceof Error ? error.message : "Connection failed"
      });
    }
  };

  // Handle logout
  const logout = () => {
    // Close WebSocket if open
    if (window.todoWebSocket) {
      window.todoWebSocket.close();
      window.todoWebSocket = undefined;
    }

    // Clear local storage
    localStorage.removeItem("todosCache");

    // Reset state
    setState({ type: "NotLoggedIn", email: "", password: "", viewPassword: false });
  };

  // Handle submitting a new task
  const submitTask = () => {
    if (state.type !== "Connected") return;

    const { inputValue } = state;
    if (!inputValue.trim()) return;

    // Handle special commands
    const firstWord = inputValue.split(" ")[0];

    switch (firstWord) {
      case "c": // collapse
        collapseDock();
        return;

      case "t": // toggle finished
        setState({
          ...state,
          showFinished: !state.showFinished,
          inputValue: ""
        });
        return;

      case "s": // succeed first task
        if (state.snapshot.live.length > 0) {
          const task = state.snapshot.live[0];
          finishTask(task.id, "Succeeded");
        }
        return;

      case "f": // fail first task
        if (state.snapshot.live.length > 0) {
          const task = state.snapshot.live[0];
          finishTask(task.id, "Failed");
        }
        return;

      case "o": // obsolete first task
        if (state.snapshot.live.length > 0) {
          const task = state.snapshot.live[0];
          finishTask(task.id, "Obsoleted");
        }
        return;

      case "r": // restore finished task
        const rMatch = inputValue.match(/^r\s*(\d+)?$/);
        if (rMatch) {
          const index = rMatch[1] ? parseInt(rMatch[1]) : 0;
          if (index < state.snapshot.finished.length) {
            restoreFinishedTask(state.snapshot.finished[index].id);
          }
        }
        return;

      case "q": // move task to end
        const qMatch = inputValue.match(/^q\s*(\d+)?$/);
        if (qMatch) {
          const index = qMatch[1] ? parseInt(qMatch[1]) : 0;
          if (state.snapshot.live.length > 1 && index < state.snapshot.live.length) {
            moveTask(state.snapshot.live[index].id, state.snapshot.live[state.snapshot.live.length - 1].id);
          }
        }
        return;

      case "mv": // move task
        const mvMatch = inputValue.match(/^mv\s+(\d+)(?:\s+(\d+))?$/);
        if (mvMatch) {
          const fromIndex = parseInt(mvMatch[1]);
          const toIndex = mvMatch[2] ? parseInt(mvMatch[2]) : 0;

          if (fromIndex !== toIndex &&
            fromIndex < state.snapshot.live.length &&
            toIndex < state.snapshot.live.length) {
            moveTask(state.snapshot.live[fromIndex].id, state.snapshot.live[toIndex].id);
          }
        }
        return;

      case "rev": // reverse tasks
        const revMatch = inputValue.match(/^rev\s+(\d+)(?:\s+(\d+))?$/);
        if (revMatch) {
          const fromIndex = parseInt(revMatch[1]);
          const toIndex = revMatch[2] ? parseInt(revMatch[2]) : 0;

          if (fromIndex !== toIndex &&
            fromIndex < state.snapshot.live.length &&
            toIndex < state.snapshot.live.length) {
            reverseTask(state.snapshot.live[fromIndex].id, state.snapshot.live[toIndex].id);
          }
        }
        return;

      default:
        // Add new task
        addNewTask(inputValue);
        return;
    }
  };

  // Send a WebSocket operation
  const sendWsOp = (op: WebsocketOpKind) => {
    if (!window.todoWebSocket || window.todoWebSocket.readyState !== WebSocket.OPEN) {
      console.error("WebSocket not connected");
      return false;
    }

    const wsOp: WebsocketOp = {
      alleged_time: currentTimeMillis(),
      kind: op
    };

    try {
      window.todoWebSocket.send(JSON.stringify(wsOp));
      return true;
    } catch (error) {
      console.error("Error sending WebSocket message:", error);
      return false;
    }
  };

  // Operations - Non-optimistic approach, waiting for server to respond
  const addNewTask = (value: string) => {
    if (state.type !== "Connected") return;

    const taskId = randomString();

    // Only clear the input if the send was successful
    if (sendWsOp({
      InsLiveTask: {
        id: taskId,
        value
      }
    })) {
      setState({
        ...state,
        inputValue: "",
      });
    }
  };

  const finishTask = (id: string, status: TaskStatus) => {
    if (state.type !== "Connected") return;

    sendWsOp({
      FinishLiveTask: {
        id,
        status
      }
    });

    // State will be updated when server responds
  };

  const restoreFinishedTask = (id: string) => {
    if (state.type !== "Connected") return;

    sendWsOp({
      RestoreFinishedTask: {
        id
      }
    });

    // State will be updated when server responds
  };

  const editTask = (id: string, value: string) => {
    if (state.type !== "Connected") return;

    // Just send to server and wait for response
    sendWsOp({
      EditLiveTask: {
        id,
        value
      }
    });
  };

  const moveTask = (fromId: string, toId: string) => {
    if (state.type !== "Connected") return;

    sendWsOp({
      MvLiveTask: {
        id_del: fromId,
        id_ins: toId
      }
    });
  };

  const reverseTask = (id1: string, id2: string) => {
    if (state.type !== "Connected") return;

    sendWsOp({
      RevLiveTask: {
        id1,
        id2
      }
    });
  };

  const setActiveTask = (id?: string) => {
    if (state.type !== "Connected") return;

    // If there was an active task being edited, save it first
    if (state.activeIdVal) {
      // Send the changes to the server
      editTask(state.activeIdVal[0], state.activeIdVal[1]);
    }

    if (!id) {
      // Just update UI state, no server interaction needed here
      setState({
        ...state,
        activeIdVal: undefined
      });
      setTimeout(() => {
        taskInputRef.current?.focus();
      }, 0);
      return;
    }

    // Find the task
    const task = state.snapshot.live.find(task => task.id === id);
    if (!task) return;

    // Just update local editing state, no server interaction needed here
    setState({
      ...state,
      activeIdVal: [id, task.value]
    });

    setTimeout(() => {
      activeTaskInputRef.current?.focus();
    }, 0);
  };

  // Render different states
  const renderAppContent = () => {
    // Not logged in, collapsed
    if (state.type === "NotLoggedIn" && !expanded) {
      return (
        <button
          className="button text-button full-size"
          onClick={expandDock}
        >
          Click to Log In
        </button>
      );
    }

    // Not logged in, expanded
    if (state.type === "NotLoggedIn" && expanded) {
      return (
        <div className="row">
          <div className="column spacing-10">
            <button onClick={collapseDock}>Collapse</button>
            <button onClick={() => setState({ ...state, viewPassword: !state.viewPassword })}>
              {state.viewPassword ? "Hide Password" : "View Password"}
            </button>
          </div>
          <div className="column spacing-10">
            <input
              ref={emailInputRef}
              type="email"
              placeholder="Email"
              value={state.email}
              onChange={e => setState({ ...state, email: e.target.value, error: undefined })}
              onKeyDown={e => e.key === "Enter" && passwordInputRef.current?.focus()}
            />
            <input
              ref={passwordInputRef}
              type={state.viewPassword ? "text" : "password"}
              placeholder="Password"
              value={state.password}
              onChange={e => setState({ ...state, password: e.target.value, error: undefined })}
              onKeyDown={e => e.key === "Enter" && state.email && state.password ? attemptLogin() : null}
            />
            <button onClick={attemptLogin}>Submit</button>
            {state.error && <div className="error">{state.error}</div>}
          </div>
        </div>
      );
    }

    // Restored cache
    if (state.type === "Restored") {
      return (
        <button
          className="button text-button full-size"
          onClick={() => connectWebsocket(state.apiKey)}
        >
          Resume Session
        </button>
      );
    }

    // Not connected, collapsed
    if (state.type === "NotConnected" && !expanded) {
      return (
        <div className="row">
          <button
            className="button text-button full-size"
            onClick={expandDock}
          >
            {state.error ?
              <span className="error">{state.error}</span> :
              "Connecting..."}
          </button>

          {state.error && (
            <button onClick={() => connectWebsocket(state.apiKey)}>
              Retry
            </button>
          )}
        </div>
      );
    }

    // Not connected, expanded
    if (state.type === "NotConnected" && expanded) {
      return (
        <div className="row spacing-10">
          <div className="column spacing-10">
            <button onClick={collapseDock}>Collapse</button>
            <button onClick={logout}>Log Out</button>
          </div>
          <div className="column spacing-10">
            {state.error ? (
              <>
                <div className="error">{state.error}</div>
                <button onClick={() => connectWebsocket(state.apiKey)}>Retry</button>
              </>
            ) : (
              <div>Connecting...</div>
            )}
          </div>
        </div>
      );
    }

    // Connected, collapsed
    if (state.type === "Connected" && !expanded) {
      const liveTasks = state.snapshot.live;

      if (liveTasks.length === 0) {
        return (
          <div className="container">
            <button onClick={expandDock}>Click to Add Task</button>
          </div>
        );
      }

      // Show the first task
      const firstTask = liveTasks[0];
      return (
        <div className="row spacing-10 full-height">
          <button
            className="button positive"
            onClick={() => finishTask(firstTask.id, "Succeeded")}
          >
            Succeeded
          </button>

          <button
            className="button text-button full-width"
            onClick={expandDock}
          >
            {firstTask.value}
          </button>

          <button
            className="button destructive"
            onClick={() => finishTask(firstTask.id, "Failed")}
          >
            Failed
          </button>

          <button
            className="button secondary"
            onClick={() => finishTask(firstTask.id, "Obsoleted")}
          >
            Obsoleted
          </button>
        </div>
      );
    }

    // Connected, expanded
    if (state.type === "Connected" && expanded) {
      const { snapshot, showFinished, activeIdVal, inputValue } = state;

      return (
        <div className="row spacing-10">
          <div className="column spacing-10">
            <button onClick={collapseDock}>Collapse</button>
            <button onClick={() => setState({ ...state, showFinished: !showFinished })}>
              {showFinished ? "Show Live Tasks" : "Show Finished Tasks"}
            </button>
            <button onClick={logout}>Log Out</button>
          </div>

          <div className="column spacing-10 full-width">
            <input
              ref={taskInputRef}
              placeholder="What needs to be done?"
              value={inputValue}
              onChange={e => setState({ ...state, inputValue: e.target.value })}
              onKeyDown={e => e.key === "Enter" && submitTask()}
              onFocus={() => setActiveTask(undefined)}
            />

            <div className="scrollable-container">
              {!showFinished ? (
                snapshot.live.length > 0 ? (
                  <div className="column padding-right-15">
                    {snapshot.live.map((task, i) => (
                      <div key={task.id} className="row spacing-10">
                        <div className="task-index">{i}|</div>

                        {activeIdVal && activeIdVal[0] === task.id ? (
                          <>
                            <button
                              className="button positive"
                              onClick={() => finishTask(task.id, "Succeeded")}
                            >
                              Task Succeeded
                            </button>

                            <input
                              ref={activeTaskInputRef}
                              value={activeIdVal[1]}
                              onChange={e => setState({
                                ...state,
                                activeIdVal: [activeIdVal[0], e.target.value]
                              })}
                              onKeyDown={e => e.key === "Enter" && setActiveTask(undefined)}
                              className="full-width"
                            />

                            <button
                              className="button destructive"
                              onClick={() => finishTask(task.id, "Failed")}
                            >
                              Task Failed
                            </button>

                            <button
                              className="button secondary"
                              onClick={() => finishTask(task.id, "Obsoleted")}
                            >
                              Task Obsoleted
                            </button>
                          </>
                        ) : (
                          <button
                            className="button text-button full-width task-button"
                            onClick={() => setActiveTask(task.id)}
                          >
                            {task.value}
                          </button>
                        )}
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className="empty-message">You have not created a task yet...</div>
                )
              ) : (
                <div className="column padding-right-15">
                  {snapshot.finished.map((task, i) => (
                    <div key={task.id} className="row spacing-10 full-width">
                      <div className="task-index">{i}|</div>

                      <div className={`status-label status-${task.status.toLowerCase()}`}>
                        {task.status.toUpperCase()}
                      </div>

                      <div className="task-text">{task.value}</div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      );
    }

    return <div>Unknown state</div>;
  };

  return (
    <main
      className={`container ${expanded ? 'expanded' : 'collapsed'}`}
      onMouseEnter={() => setFocused(true)}
      onMouseLeave={() => setFocused(false)}
    >
      {renderAppContent()}
    </main>
  );
}

export default App;