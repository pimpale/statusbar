import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { Button, Form, Container, Row, Col, Stack, Badge, ListGroup } from 'react-bootstrap';
import {
  StateSnapshot,
  TaskStatus,
  WebsocketOp,
  WebsocketOpKind,
  WebsocketOpSchema,
  ServerInfoSchema,
  ServerInfo,
  AppState,
  TodosCache
} from "./types";
import {
  applyOperation,
  currentTimeMillis,
  randomString,
  parseRestoreCommand,
  parseMoveToEndCommand,
  parseMoveCommand,
  parseReverseCommand
} from "./utils/taskUtils";
import {
  saveCache,
  loadCache,
  clearCache
} from "./utils/storageUtils";

// Extend Window interface to store WebSocket
declare global {
  interface Window {
    todoWebSocket?: WebSocket;
  }
}

function App() {
  // Main app state
  const [state, setState] = useState<AppState>({
    type: "NotLoggedIn",
    email: "",
    password: "",
    viewPassword: false
  });

  // UI state
  const [expanded, setExpanded_raw] = useState(false);
  const [focused, setFocused] = useState(false);
  const [windowFocused, setWindowFocused_raw] = useState(false);

  // Default server URL
  const defaultServerUrl = "http://localhost:8080/public/";
  const [serverApiUrl, setServerApiUrl] = useState(defaultServerUrl);

  // Refs for inputs
  const emailInputRef = useRef<HTMLInputElement>(null);
  const passwordInputRef = useRef<HTMLInputElement>(null);
  const taskInputRef = useRef<HTMLInputElement>(null);
  const activeTaskInputRef = useRef<HTMLInputElement>(null);

  // Create the new setWindowFocused function that handles the Tauri invoke
  const setWindowFocused = async (newState: boolean) => {
    try {
      console.debug(`Attempting to ${newState ? 'focus' : 'unfocus'} window...`);
      if (newState) {
        await invoke('focus_window');
      } else {
        await invoke('unfocus_window');
      }
      setWindowFocused_raw(newState);
    } catch (error) {
      console.error('Failed to change window focus state:', error);
    }
  };

  // Create the new setExpand function that handles the Tauri invoke
  const setExpand = async (newState: boolean) => {
    try {
      console.debug(`Attempting to ${newState ? 'expand' : 'unexpand'} window...`);
      if (newState) {
        await invoke('expand_window');
      } else {
        await invoke('unexpand_window');
      }
      setExpanded_raw(newState);
    } catch (error) {
      console.error('Failed to change window size:', error);
    }
  };

  // Load cached data on mount
  useEffect(() => {
    const cache = loadCache();
    if (cache) {
      setServerApiUrl(cache.serverApiUrl);
      setState({ type: "Restored", apiKey: cache.apiKey });
    }
  }, []);

  // Update all the existing code to use setWindowFocused instead of setGrabbed
  const handleMouseEnter = async () => {
    setFocused(true);
    if (expanded && !windowFocused) {
      await setWindowFocused(true);
    }
  };

  const handleMouseLeave = async () => {
    setFocused(false);
    if (windowFocused) {
      await setWindowFocused(false);
    }
  };

  const expandDock = async () => {
    await setExpand(true);
    if (focused && !windowFocused) {
      await setWindowFocused(true);
    }

    // Focus the appropriate input
    setTimeout(() => {
      if (state.type === "NotLoggedIn") {
        emailInputRef.current?.focus();
      } else if (state.type === "Connected") {
        taskInputRef.current?.focus();
      }
    }, 0);
  };

  const collapseDock = async () => {
    await setExpand(false);
    if (windowFocused) {
      await setWindowFocused(false);
    }

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

      // Save to cache
      saveCache({
        serverApiUrl,
        apiKey: apiKeyData.key
      });

      // Connect to websocket
      connectWebsocket(apiKeyData.key);
    } catch (error) {
      setState({
        ...state,
        error: error instanceof Error ? error.message : "Unknown error"
      });
    }
  };

  // Modify connectWebsocket to close existing connection first
  const connectWebsocket = async (apiKey: string) => {
    // Set state to connecting
    setState({ type: "NotConnected", apiKey });

    // Close existing WebSocket if any
    if (window.todoWebSocket) {
      window.todoWebSocket.close(1000, "New connection requested");
      window.todoWebSocket = undefined;
    }

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

      // Handle errors
      ws.addEventListener('error', (error) => {
        console.error('WebSocket error:', error);
        // Close with code 1011 (Internal Error)
        if (ws.readyState === WebSocket.OPEN) {
          ws.close(1011, "WebSocket error occurred");
        }
        // Don't set state here, let the close handler handle it
      });

      // Handle close
      ws.addEventListener('close', (event) => {
        console.log('WebSocket closed:', event.code, event.reason);

        if (event.reason === 'Unauthorized' || !loadCache()) {
          const error = event.reason === 'Unauthorized' ? 'Session expired. Please log in again.' : undefined;

          // If unauthorized or no cache (logged out), go back to login
          setState({
            type: "NotLoggedIn",
            email: "",
            password: "",
            viewPassword: false,
            error
          });
        } else {
          setState({
            type: "NotConnected",
            apiKey,
            error: event.reason || "Connection closed"
          });
        }
      });

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
    // Close WebSocket if open with a proper code and reason
    if (window.todoWebSocket && window.todoWebSocket.readyState === WebSocket.OPEN) {
      // 1000 is normal closure, clean exit
      window.todoWebSocket.close(1000, "User logged out");
      window.todoWebSocket = undefined;
    }

    // Clear cache
    clearCache();

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
          setState({
            ...state,
            inputValue: ""
          });
        }
        return;

      case "f": // fail first task
        if (state.snapshot.live.length > 0) {
          const task = state.snapshot.live[0];
          finishTask(task.id, "Failed");
          setState({
            ...state,
            inputValue: ""
          });
        }
        return;

      case "o": // obsolete first task
        if (state.snapshot.live.length > 0) {
          const task = state.snapshot.live[0];
          finishTask(task.id, "Obsoleted");
          setState({
            ...state,
            inputValue: ""
          });
        }
        return;

      case "r": // restore finished task
        const restoreIndex = parseRestoreCommand(inputValue);
        if (restoreIndex !== null && restoreIndex < state.snapshot.finished.length) {
          restoreFinishedTask(state.snapshot.finished[restoreIndex].id);
          setState({
            ...state,
            inputValue: ""
          });
        }
        return;

      case "q": // move task to end
        const moveToEndIndex = parseMoveToEndCommand(inputValue);
        if (moveToEndIndex !== null &&
          state.snapshot.live.length > 1 &&
          moveToEndIndex < state.snapshot.live.length) {
          moveTask(
            state.snapshot.live[moveToEndIndex].id,
            state.snapshot.live[state.snapshot.live.length - 1].id
          );
          setState({
            ...state,
            inputValue: ""
          });
        }
        return;

      case "mv": // move task
        const moveIndices = parseMoveCommand(inputValue);
        if (moveIndices !== null) {
          const [fromIndex, toIndex] = moveIndices;

          if (fromIndex !== toIndex &&
            fromIndex < state.snapshot.live.length &&
            toIndex < state.snapshot.live.length) {
            moveTask(state.snapshot.live[fromIndex].id, state.snapshot.live[toIndex].id);
            setState({
              ...state,
              inputValue: ""
            });
          }
        }
        return;

      case "rev": // reverse tasks
        const reverseIndices = parseReverseCommand(inputValue);
        if (reverseIndices !== null) {
          const [fromIndex, toIndex] = reverseIndices;

          if (fromIndex !== toIndex &&
            fromIndex < state.snapshot.live.length &&
            toIndex < state.snapshot.live.length) {
            reverseTask(state.snapshot.live[fromIndex].id, state.snapshot.live[toIndex].id);
            setState({
              ...state,
              inputValue: ""
            });
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
        <Button
          variant="link"
          className="w-100 h-100"
          onClick={expandDock}
        >
          Click to Log In
        </Button>
      );
    }

    // Not logged in, expanded
    if (state.type === "NotLoggedIn" && expanded) {
      return (
          <Row className="g-2">
            <Col xs="auto">
              <Stack gap={2}>
                <Button variant="secondary" onClick={collapseDock}>Collapse</Button>
                <Button variant="secondary" onClick={() => setState({ ...state, viewPassword: !state.viewPassword })}>
                  {state.viewPassword ? "Hide Password" : "View Password"}
                </Button>
              </Stack>
            </Col>
            <Col>
              <Stack gap={2}>
                <Form.Control
                  ref={emailInputRef}
                  type="email"
                  placeholder="Email"
                  value={state.email}
                  onChange={e => setState({ ...state, email: e.target.value, error: undefined })}
                  onKeyDown={e => e.key === "Enter" && passwordInputRef.current?.focus()}
                />
                <Form.Control
                  ref={passwordInputRef}
                  type={state.viewPassword ? "text" : "password"}
                  placeholder="Password"
                  value={state.password}
                  onChange={e => setState({ ...state, password: e.target.value, error: undefined })}
                  onKeyDown={e => e.key === "Enter" && state.email && state.password ? attemptLogin() : null}
                />
                <Button variant="primary" onClick={attemptLogin}>Submit</Button>
                {state.error && <div className="text-danger">{state.error}</div>}
              </Stack>
            </Col>
          </Row>
      );
    }

    // Restored cache
    if (state.type === "Restored") {
      return (
        <Button
          variant="link"
          className="w-100 h-100"
          onClick={() => connectWebsocket(state.apiKey)}
        >
          Resume Session
        </Button>
      );
    }

    // Not connected, collapsed
    if (state.type === "NotConnected" && !expanded) {
      return (
          <Row className="g-2">
            <Col>
              <Button
                variant="link"
                className="w-100 h-100"
                onClick={expandDock}
              >
                {state.error ?
                  <span className="text-danger">{state.error}</span> :
                  "Connecting..."}
              </Button>
            </Col>
            {state.error && (
              <Col xs="auto">
                <Button variant="secondary" onClick={() => connectWebsocket(state.apiKey)}>
                  Retry
                </Button>
              </Col>
            )}
          </Row>
      );
    }

    // Not connected, expanded
    if (state.type === "NotConnected" && expanded) {
      return (
          <Row className="g-2">
            <Col xs="auto">
              <Stack gap={2}>
                <Button variant="secondary" onClick={collapseDock}>Collapse</Button>
                <Button variant="secondary" onClick={logout}>Log Out</Button>
              </Stack>
            </Col>
            <Col>
              <Stack gap={2}>
                {state.error ? (
                  <>
                    <div className="text-danger">{state.error}</div>
                    <Button variant="primary" onClick={() => connectWebsocket(state.apiKey)}>Retry</Button>
                  </>
                ) : (
                  <div>Connecting...</div>
                )}
              </Stack>
            </Col>
          </Row>
      );
    }

    // Connected, collapsed
    if (state.type === "Connected" && !expanded) {
      const liveTasks = state.snapshot.live;

      if (liveTasks.length === 0) {
        return (
            <Button variant="link" className="w-100" onClick={expandDock}>Click to Add Task</Button>
        );
      }

      // Show the first task
      const firstTask = liveTasks[0];
      return (
          <Row className="g-2 h-100 align-items-center">
            <Col xs="auto">
              <Button
                variant="success"
                onClick={() => finishTask(firstTask.id, "Succeeded")}
              >
                Succeeded
              </Button>
            </Col>

            <Col>
              <Button
                variant="link"
                className="w-100 text-start"
                onClick={expandDock}
              >
                {firstTask.value}
              </Button>
            </Col>

            <Col xs="auto">
              <Button
                variant="danger"
                onClick={() => finishTask(firstTask.id, "Failed")}
              >
                Failed
              </Button>
            </Col>

            <Col xs="auto">
              <Button
                variant="secondary"
                onClick={() => finishTask(firstTask.id, "Obsoleted")}
              >
                Obsoleted
              </Button>
            </Col>
          </Row>
      );
    }

    // Connected, expanded
    if (state.type === "Connected" && expanded) {
      const { snapshot, showFinished, activeIdVal, inputValue } = state;

      return (
          <Row className="g-2">
            <Col xs="auto">
              <Stack gap={2}>
                <Button variant="secondary" onClick={collapseDock}>Collapse</Button>
                <Button variant="secondary" onClick={() => setState({ ...state, showFinished: !showFinished })}>
                  {showFinished ? "Show Live Tasks" : "Show Finished Tasks"}
                </Button>
                <Button variant="secondary" onClick={logout}>Log Out</Button>
              </Stack>
            </Col>

            <Col>
              <Stack gap={2}>
                <Form.Control
                  ref={taskInputRef}
                  placeholder="What needs to be done?"
                  value={inputValue}
                  onChange={e => setState({ ...state, inputValue: e.target.value })}
                  onKeyDown={e => e.key === "Enter" && submitTask()}
                  onFocus={() => setActiveTask(undefined)}
                />
                  {!showFinished ? (
                    snapshot.live.length > 0 ? (
                      <ListGroup>
                        {snapshot.live.map((task, i) => (
                          <ListGroup.Item key={task.id} className="p-2">
                            <Row className="g-2 align-items-center">
                              <Col xs="auto" style={{ fontSize: '1.5rem', minWidth: '3rem' }}>
                                {i}|
                              </Col>

                              {activeIdVal && activeIdVal[0] === task.id ? (
                                <>
                                  <Col xs="auto">
                                    <Button
                                      variant="success"
                                      onClick={() => finishTask(task.id, "Succeeded")}
                                    >
                                      Task Succeeded
                                    </Button>
                                  </Col>

                                  <Col>
                                    <Form.Control
                                      ref={activeTaskInputRef}
                                      value={activeIdVal[1]}
                                      onChange={e => setState({
                                        ...state,
                                        activeIdVal: [activeIdVal[0], e.target.value]
                                      })}
                                      onKeyDown={e => e.key === "Enter" && setActiveTask(undefined)}
                                    />
                                  </Col>

                                  <Col xs="auto">
                                    <Button
                                      variant="danger"
                                      onClick={() => finishTask(task.id, "Failed")}
                                    >
                                      Task Failed
                                    </Button>
                                  </Col>

                                  <Col xs="auto">
                                    <Button
                                      variant="secondary"
                                      onClick={() => finishTask(task.id, "Obsoleted")}
                                    >
                                      Task Obsoleted
                                    </Button>
                                  </Col>
                                </>
                              ) : (
                                <Col>
                                  <Button
                                    variant="link"
                                    className="w-100 text-start p-0"
                                    onClick={() => setActiveTask(task.id)}
                                  >
                                    {task.value}
                                  </Button>
                                </Col>
                              )}
                            </Row>
                          </ListGroup.Item>
                        ))}
                      </ListGroup>
                    ) : (
                      <div className="text-muted fs-4 p-3">You have not created a task yet...</div>
                    )
                  ) : (
                    snapshot.finished.length > 0 ? (
                      <ListGroup>
                        {snapshot.finished.map((task, i) => (
                          <ListGroup.Item key={task.id} className="p-2">
                            <Row className="g-2 align-items-center">
                              <Col xs="auto" style={{ fontSize: '1.5rem', minWidth: '3rem' }}>
                                {i}|
                              </Col>

                              <Col xs="auto">
                                <Badge 
                                  bg={
                                    task.status === "Succeeded" ? "success" :
                                    task.status === "Failed" ? "danger" :
                                    "secondary"
                                  }
                                  style={{ minWidth: '80px' }}
                                >
                                  {task.status.toUpperCase()}
                                </Badge>
                              </Col>

                              <Col className="d-flex align-items-center">
                                {task.value}
                              </Col>
                            </Row>
                          </ListGroup.Item>
                        ))}
                      </ListGroup>
                    ) : (
                      <div className="text-muted fs-4 p-3">No finished tasks yet...</div>
                    )
                  )}
              </Stack>
            </Col>
          </Row>
      );
    }

    return <div>Unknown state</div>;
  };

  return (
    <Container fluid
      style={{
        height: "100vh",
      }}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      onMouseMove={handleMouseEnter}
    >
      {renderAppContent()}
    </Container>
  );
}

export default App;