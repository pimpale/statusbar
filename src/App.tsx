import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { Button, Form, Container, Row, Col, Stack, Badge, ListGroup, InputGroup, Tabs, Tab, OverlayTrigger, Tooltip } from 'react-bootstrap';
import DatePicker from "react-datepicker";
import "react-datepicker/dist/react-datepicker.css";
import {
  StateSnapshot,
  TaskStatus,
  WebsocketOp,
  WebsocketOpKind,
  WebsocketOpSchema,
  ServerInfoSchema,
  ServerInfo,
  AppState,
  TodosCache,
  ViewType,
  LiveTask,
  FinishedTask
} from "./types";
import {
  applyOperation,
  currentTimeMillis,
  randomString,
  parseRestoreCommand,
  parseMoveToEndCommand,
  parseMoveCommand,
  parseReverseCommand,
  parseDueCommand
} from "./utils/taskUtils";
import {
  saveCache,
  loadCache,
  clearCache
} from "./utils/storageUtils";
import { format, fromUnixTime, getUnixTime } from 'date-fns';


// Extend Window interface to store WebSocket
declare global {
  interface Window {
    todoWebSocket?: WebSocket;
  }
}

// Add interfaces at the top of the file
interface LoginScreenProps {
  state: Extract<AppState, { type: "NotLoggedIn" }>;
  expanded: boolean;
  emailInputRef: React.RefObject<HTMLInputElement>;
  passwordInputRef: React.RefObject<HTMLInputElement>;
  expandDock: () => void;
  collapseDock: () => void;
  setState: (updater: (state: AppState) => AppState) => void;
  attemptLogin: (email: string, password: string) => void;
  serverApiUrl: string;
  setServerApiUrl: (url: string) => void;
}

interface ConnectedScreenProps {
  state: Extract<AppState, { type: "Connected" }>;
  expanded: boolean;
  taskInputRef: React.RefObject<HTMLInputElement>;
  activeTaskInputRef: React.RefObject<HTMLInputElement>;
  expandDock: () => void;
  collapseDock: () => void;
  setState: (updater: (state: AppState) => AppState) => void;
  logout: () => void;
  submitTask: () => void;
  finishTask: (id: string, status: TaskStatus) => void;
  setActiveTask: (id?: string) => void;
  editTask: (id: string, value: string, deadline: number | null) => void;
}

interface RestoredScreenProps {
  state: Extract<AppState, { type: "Restored" }>;
  connectWebsocket: (apiKey: string) => void;
}

interface NotConnectedScreenProps {
  state: Extract<AppState, { type: "NotConnected" }>;
  expanded: boolean;
  expandDock: () => void;
  connectWebsocket: (apiKey: string) => void;
}

interface DeadlineBadgeProps {
  deadline: number | null;
  countdown?: boolean;
  className?: string;
}

interface OverdueTasksScreenProps {
  tasks: LiveTask[];
  finishTask: (id: string, status: TaskStatus) => void;
}

interface FinishedTasksScreenProps {
  tasks: FinishedTask[];
}

interface PreferencesScreenProps {
  state: Extract<AppState, { type: "Connected" }>;
  setState: (updater: (state: AppState) => AppState) => void;
}

interface LiveTasksScreenProps {
  activeIdVal?: [string, string, number | null];
  activeTaskInputRef: React.RefObject<HTMLInputElement>;
  setActiveTask: (id?: string) => void;
  editTask: (id: string, value: string, deadline: number | null) => void;
  finishTask: (id: string, status: TaskStatus) => void;
  state: Extract<AppState, { type: "Connected" }>;
  setState: (updater: (state: AppState) => AppState) => void;
}

interface TabTitleProps {
  title: string;
  disabled?: boolean;
  tooltip?: string;
}

interface TooltipButtonProps {
  variant?: string;
  onClick?: () => void;
  disabled?: boolean;
  tooltip?: string;
  children: React.ReactNode;
}

const TabTitle: React.FC<TabTitleProps> = ({ title, disabled, tooltip }) => {
  if (!disabled || !tooltip) {
    return <>{title}</>;
  }

  return (
    <div style={{ display: 'inline-block', cursor: 'not-allowed' }}>
      <OverlayTrigger
        placement="bottom"
        overlay={<Tooltip>{tooltip}</Tooltip>}
        trigger={['hover', 'focus']}
      >
        <div style={{ display: 'inline-block' }}>{title}</div>
      </OverlayTrigger>
    </div>
  );
};

const TooltipButton: React.FC<TooltipButtonProps> = ({ variant = "primary", onClick, disabled, tooltip, children }) => {
  if (!disabled || !tooltip) {
    return (
      <Button variant={variant} onClick={onClick}>
        {children}
      </Button>
    );
  }

  return (
    <OverlayTrigger
      placement="right"
      overlay={<Tooltip>{tooltip}</Tooltip>}
      trigger={['hover', 'focus']}
    >
      <div style={{ display: 'inline-block', cursor: 'not-allowed' }}>
        <Button variant={variant} style={{ pointerEvents: 'none' }} disabled={true}>
          {children}
        </Button>
      </div>
    </OverlayTrigger>
  );
};

const DeadlineBadge: React.FC<DeadlineBadgeProps> = ({ deadline, countdown = false, className = "" }) => {
  const [currentTime, setCurrentTime] = useState(Date.now() / 1000);

  useEffect(() => {
    if (!countdown || !deadline) return;

    const interval = setInterval(() => {
      setCurrentTime(Date.now() / 1000);
    }, 1000);

    return () => clearInterval(interval);
  }, [countdown, deadline]);

  if (deadline === null) return null;

  const formatDeadline = (timestamp: number) => {
    const date = fromUnixTime(timestamp);
    const today = new Date();

    if (countdown) {
      const diffSeconds = Math.floor(timestamp - currentTime);
      if (diffSeconds < 0) {
        return "Overdue";
      }

      const days = Math.floor(diffSeconds / (24 * 60 * 60));
      const hours = Math.floor((diffSeconds % (24 * 60 * 60)) / (60 * 60));
      const minutes = Math.floor((diffSeconds % (60 * 60)) / 60);
      const seconds = diffSeconds % 60;

      const daysPadded = days.toString().padStart(2, ' ');
      const hoursPadded = hours.toString().padStart(2, ' ');
      const minutesPadded = minutes.toString().padStart(2, ' ');
      const secondsPadded = seconds.toString().padStart(2, ' ');

      if (days > 0) {
        return `${daysPadded}d ${hoursPadded}h left`;
      } else if (hours > 0) {
        return `${hoursPadded}h ${minutesPadded}m left`;
      } else if (minutes > 0) {
        return `${minutesPadded}m ${secondsPadded}s left`;
      } else {
        return `${secondsPadded}s left`;
      }
    }

    // If the date is today, only show the time
    if (format(date, 'yyyy-MM-dd') === format(today, 'yyyy-MM-dd')) {
      return format(date, 'h:mm a');
    }

    // Otherwise show both date and time
    return format(date, 'MMM d, yyyy h:mm a');
  };

  const getBadgeVariant = (timestamp: number) => {
    const date = fromUnixTime(timestamp);
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());

    // If the deadline is in the past
    if (date < now) {
      return "danger";  // red
    }

    // If the deadline is today
    if (format(date, 'yyyy-MM-dd') === format(today, 'yyyy-MM-dd')) {
      return "warning";  // yellow
    }

    // If the deadline is in the future
    return "success";  // green
  };

  return (
    <Badge bg={getBadgeVariant(deadline)} className={className}>
      {countdown ? <pre children={formatDeadline(deadline)} className="m-0" /> : formatDeadline(deadline)}
    </Badge>
  );
}

// Add component definitions before App function
const LoginScreen: React.FC<LoginScreenProps> = ({
  state,
  expanded,
  emailInputRef,
  passwordInputRef,
  expandDock,
  collapseDock,
  setState,
  attemptLogin,
  serverApiUrl,
  setServerApiUrl
}) => {
  const defaultServerUrl = "http://localhost:8080/public/";
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [viewPassword, setViewPassword] = useState(false);

  if (!expanded) {
    return (
      <Button variant="link" className="w-100 h-100" onClick={expandDock}>
        Click to Log In
      </Button>
    );
  }

  return (
    <Row className="g-2 py-3">
      <Col xs="auto">
        <Stack gap={2}>
          <Button variant="secondary" onClick={collapseDock}>Collapse</Button>
        </Stack>
      </Col>
      <Col>
        <Stack gap={2}>
          <h5>Login</h5>
          <Form>
            <Form.Group className="mb-2">
              <Form.Label>Email</Form.Label>
              <Form.Control
                ref={emailInputRef}
                type="email"
                value={email}
                onChange={e => {
                  setEmail(e.target.value);
                  setState(prevState => ({ ...prevState, error: undefined }));
                }}
                onKeyDown={e => e.key === "Enter" && passwordInputRef.current?.focus()}
              />
            </Form.Group>

            <Form.Group className="mb-2">
              <Form.Label>Password</Form.Label>
              <InputGroup>
                <Form.Control
                  ref={passwordInputRef}
                  type={viewPassword ? "text" : "password"}
                  value={password}
                  onChange={e => {
                    setPassword(e.target.value);
                    setState(prevState => ({ ...prevState, error: undefined }));
                  }}
                  onKeyDown={e => e.key === "Enter" && email && password ? attemptLogin(email, password) : null}
                />
                <Button
                  variant="outline-secondary"
                  onClick={() => setViewPassword(!viewPassword)}
                >
                  <i className={viewPassword ? "bi bi-eye-slash-fill" : "bi bi-eye-fill"}></i>
                </Button>
              </InputGroup>
            </Form.Group>
            <details className="mb-2">
              <summary style={{ cursor: 'pointer', userSelect: 'none' }}>Server API URL</summary>
              <Form.Control
                type="text"
                placeholder={defaultServerUrl}
                value=""
                onChange={e => {
                  setServerApiUrl(e.target.value);
                  setState(prevState => ({ ...prevState, error: undefined }));
                }}
                className="mt-2"
              />
            </details>
            <Button variant="primary" onClick={() => attemptLogin(email, password)}>Submit</Button>
            {state.error && <div className="text-danger">{state.error}</div>}
          </Form>
        </Stack>
      </Col>
    </Row>
  );
};

const OverdueTasksScreen: React.FC<OverdueTasksScreenProps> = ({
  tasks,
  finishTask
}) => {
  if (tasks.length === 0) {
    return <div className="text-muted fs-4 p-3">No overdue tasks</div>;
  }

  return (
    <ListGroup>
      {tasks.map((task, i) => (
        <ListGroup.Item key={task.id} className="p-2">
          <Row className="g-2 align-items-center">
            <Col xs="auto" style={{ fontSize: '1.5rem', minWidth: '3rem' }}>
              {i}|
            </Col>
            <Col>
              {task.value}
            </Col>
            <Col>
              <DeadlineBadge deadline={task.deadline} countdown={true} />
            </Col>
            <Col xs="auto">
              <Button variant="success" onClick={() => finishTask(task.id, "Succeeded")}>
                Succeeded
              </Button>
            </Col>
            <Col xs="auto">
              <Button variant="danger" onClick={() => finishTask(task.id, "Failed")}>
                Failed
              </Button>
            </Col>
            <Col xs="auto">
              <Button variant="secondary" onClick={() => finishTask(task.id, "Obsoleted")}>
                Obsoleted
              </Button>
            </Col>
          </Row>
        </ListGroup.Item>
      ))}
    </ListGroup>
  );
};

const LiveTasksScreen: React.FC<LiveTasksScreenProps> = ({
  activeIdVal,
  activeTaskInputRef,
  setActiveTask,
  editTask,
  finishTask,
  state,
  setState
}) => {
  const tasks = state.snapshot.live;
  if (tasks.length === 0) {
    return <div className="text-muted fs-4 p-3">You have not created a task yet...</div>;
  }

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (activeIdVal) {
      console.log("handleInputChange", e.target.value);
      setState(prevState => ({
        ...prevState,
        activeIdVal: [activeIdVal[0], e.target.value, activeIdVal[2]]
      }));
    }
  };

  const handleDateChange = (date: Date | null, task: LiveTask) => {
    const deadline = date ? getUnixTime(date) : null;
    setState(prevState => ({
      ...prevState,
      activeIdVal: [task.id, task.value, deadline]
    }));
    editTask(task.id, task.value, deadline);
  };

  return (
    <ListGroup>
      {tasks.map((task, i) => {
        const isActive = activeIdVal && activeIdVal[0] === task.id;
        return (
          <ListGroup.Item key={task.id} className="p-2" onClick={isActive ? undefined : () => setActiveTask(task.id)}>
            <Row className="g-2 align-items-center">
              <Col xs="auto" style={{ fontSize: '1.5rem', minWidth: '3rem' }}>
                {i}|
              </Col>

              {isActive ? (
                <>
                  <Col xs="auto">
                    <Button variant="success" onClick={() => finishTask(task.id, "Succeeded")}>
                      Task Succeeded
                    </Button>
                  </Col>

                  <Col>
                    <Form.Control
                      ref={activeTaskInputRef}
                      value={activeIdVal[1]}
                      onChange={handleInputChange}
                      onKeyDown={e => e.key === "Enter" && setActiveTask(undefined)}
                    />
                  </Col>
                  <Col>
                    <DatePicker
                      showIcon
                      selected={activeIdVal[2] !== null ? fromUnixTime(activeIdVal[2]) : null}
                      onChange={(date: Date | null) => handleDateChange(date, task)}
                      onKeyDown={e => e.key === "Enter" && setActiveTask(undefined)}
                      showTimeSelect
                      timeFormat="h:mm aa"
                      timeIntervals={15}
                      dateFormat="MMM d, yyyy h:mm aa"
                      minDate={new Date()}
                      filterTime={(time) => {
                        const selected = new Date(time);
                        const now = new Date();
                        // If it's today, only allow future times
                        if (format(selected, 'yyyy-MM-dd') === format(now, 'yyyy-MM-dd')) {
                          return selected > now;
                        }
                        // For future dates, allow all times
                        return true;
                      }}
                      isClearable
                      placeholderText="Select date and time"
                      className="form-control"
                      wrapperClassName="w-100"
                      icon={<i className="bi bi-calendar" style={{ fontSize: '0.8rem' }} />}
                    />
                  </Col>
                  <Col xs="auto">
                    <Button variant="dark" onClick={() => setActiveTask(undefined)}>
                      Done
                    </Button>
                  </Col>
                  <Col xs="auto">
                    <Button variant="danger" onClick={() => finishTask(task.id, "Failed")}>
                      Task Failed
                    </Button>
                  </Col>

                  <Col xs="auto">
                    <Button variant="secondary" onClick={() => finishTask(task.id, "Obsoleted")}>
                      Task Obsoleted
                    </Button>
                  </Col>
                </>
              ) : (
                <>
                  <Col>
                    {task.value}
                  </Col>
                  <Col>
                    <DeadlineBadge deadline={task.deadline} />
                  </Col>
                </>
              )}
            </Row>
          </ListGroup.Item>
        );
      })}
    </ListGroup>
  );
};

const FinishedTasksScreen: React.FC<FinishedTasksScreenProps> = ({
  tasks
}) => {
  if (tasks.length === 0) {
    return <div className="text-muted fs-4 p-3">No finished tasks yet...</div>;
  }

  return (
    <ListGroup>
      {tasks.map((task, i) => (
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
                style={{ width: '6rem' }}
              >
                {task.status.toUpperCase()}
              </Badge>
            </Col>

            <Col>
              {task.value}
            </Col>
            <Col>
              <DeadlineBadge deadline={task.deadline} />
            </Col>
          </Row>
        </ListGroup.Item>
      ))}
    </ListGroup>
  );
};

const PreferencesScreen: React.FC<PreferencesScreenProps> = ({
  state,
  setState
}) => {
  const { preferences } = state;

  const handleVocalEnabledChange = (enabled: boolean) => {
    setState(prevState => {
      if (prevState.type !== "Connected") return prevState;
      return {
        ...prevState,
        preferences: {
          ...prevState.preferences,
          vocalEnabled: enabled
        }
      };
    });
  };

  const handleVocalFrequencyChange = (frequency: number) => {
    setState(prevState => {
      if (prevState.type !== "Connected") return prevState;
      return {
        ...prevState,
        preferences: {
          ...prevState.preferences,
          vocalFrequency: frequency
        }
      };
    });
  };

  // Convert frequency in seconds to minutes for display
  const frequencyInMinutes = Math.round(preferences.vocalFrequency / 60);

  return (
    <Container className="py-3">
      <h4>Vocal Reminders</h4>
      <Form.Check
        type="checkbox"
        id="vocal-enabled"
        label="Enable vocal reminders (requires espeak-ng to be installed)"
        checked={preferences.vocalEnabled}
        onChange={(e) => handleVocalEnabledChange(e.target.checked)}
        className="mb-3"
      />
      <Form.Group>
        <Form.Label>
          Frequency: {frequencyInMinutes} minute{frequencyInMinutes !== 1 ? 's' : ''}
        </Form.Label>
        <Form.Range
          min={1}
          max={60}
          value={frequencyInMinutes}
          onChange={(e) => handleVocalFrequencyChange(parseInt(e.target.value) * 60)}
          disabled={!preferences.vocalEnabled}
        />
      </Form.Group>
    </Container>
  );
};

const ConnectedScreen: React.FC<ConnectedScreenProps> = ({
  state,
  expanded,
  taskInputRef,
  activeTaskInputRef,
  expandDock,
  collapseDock,
  setState,
  logout,
  submitTask,
  finishTask,
  setActiveTask,
  editTask
}) => {
  const { snapshot, viewType, activeIdVal, inputValue } = state;

  // Get overdue tasks
  const overdueTasks: LiveTask[] = snapshot.live.filter(task => {
    if (!task.deadline) return false;
    return (Date.now() / 1000) > task.deadline;
  });

  // Track previous overdue tasks count to detect when last overdue task is finished
  const prevOverdueTasksCountRef = useRef(overdueTasks.length);

  // Effect to switch to Live view when last overdue task is finished
  useEffect(() => {
    // If we were on the overdue tab and all overdue tasks are now gone
    if (viewType === ViewType.Overdue &&
      overdueTasks.length === 0 &&
      prevOverdueTasksCountRef.current > 0) {
      // Switch to live tasks view
      setState(prevState => ({
        ...prevState,
        viewType: ViewType.Live
      }));

      // Focus the task input field
      setTimeout(() => {
        taskInputRef.current?.focus();
      }, 0);
    }

    // Update the reference for next render
    prevOverdueTasksCountRef.current = overdueTasks.length;
  }, [overdueTasks.length, viewType]);

  if (!expanded) {
    const liveTasks = snapshot.live;

    if (liveTasks.length === 0) {
      return (
        <Button variant="link" className="w-100" onClick={expandDock}>
          Click to Add Task
        </Button>
      );
    }

    // Show the first task
    const firstTask = liveTasks[0];
    return (
      <div className="d-flex gap-2 py-3 h-100">
        <Button variant="success" className="h-100" onClick={() => finishTask(firstTask.id, "Succeeded")}>
          Succeeded
        </Button>
        <button className="btn flex-grow-1 h-100 fs-4" onClick={expandDock}>
          {firstTask.value}
          <DeadlineBadge deadline={firstTask.deadline} countdown={true} className="ms-5" />
        </button>
        <Button variant="danger" className="h-100" onClick={() => finishTask(firstTask.id, "Failed")}>
          Failed
        </Button>
        <Button variant="secondary" className="h-100" onClick={() => finishTask(firstTask.id, "Obsoleted")}>
          Obsoleted
        </Button>
      </div>
    );
  }

  return (
    <Row className="g-2 py-3">
      <Col xs="auto">
        <Stack gap={2}>
          <TooltipButton
            variant="secondary"
            onClick={overdueTasks.length > 0 ? undefined : collapseDock}
            disabled={overdueTasks.length > 0}
            tooltip={overdueTasks.length > 0 ? "Please resolve overdue tasks first" : undefined}
          >
            Collapse
          </TooltipButton>
          <Button variant="secondary" onClick={logout}>Log Out</Button>
        </Stack>
      </Col>
      <Col>
        <Stack gap={2}>
          <Tabs
            activeKey={viewType}
            onSelect={(k) => {
              if (k === ViewType.Live || k === ViewType.Overdue || k === ViewType.Finished || k === ViewType.Preferences) {
                // Only allow switching if there are no overdue tasks, or if switching to overdue tasks tab
                if (overdueTasks.length === 0 || k === ViewType.Overdue) {
                  setState(prevState => ({ ...prevState, viewType: k }));
                  if (k === ViewType.Live) {
                    setTimeout(() => {
                      taskInputRef.current?.focus();
                    }, 0);
                  }
                }
              }
            }}
          >
            <Tab
              eventKey={ViewType.Live}
              title={
                <TabTitle
                  title="Live Tasks"
                  disabled={overdueTasks.length > 0}
                  tooltip={overdueTasks.length > 0 ? "Please resolve overdue tasks first" : undefined}
                />
              }
              tabClassName={overdueTasks.length > 0 ? "text-muted" : ""}
            >
              <Form.Control
                ref={taskInputRef}
                placeholder="What needs to be done?"
                value={inputValue}
                onChange={e => setState(prevState => ({ ...prevState, inputValue: e.target.value }))}
                onKeyDown={e => e.key === "Enter" && submitTask()}
                onFocus={() => setActiveTask(undefined)}
              />
              <LiveTasksScreen
                activeIdVal={activeIdVal}
                activeTaskInputRef={activeTaskInputRef}
                setActiveTask={setActiveTask}
                editTask={editTask}
                finishTask={finishTask}
                state={state}
                setState={setState}
              />
            </Tab>
            <Tab
              eventKey={ViewType.Overdue}
              title={`Overdue Tasks (${overdueTasks.length})`}
              className="text-danger"
            >
              <OverdueTasksScreen
                tasks={overdueTasks}
                finishTask={finishTask}
              />
            </Tab>
            <Tab
              eventKey={ViewType.Finished}
              title={
                <TabTitle
                  title="Finished Tasks"
                  disabled={overdueTasks.length > 0}
                  tooltip={overdueTasks.length > 0 ? "Please resolve overdue tasks first" : undefined}
                />
              }
              tabClassName={overdueTasks.length > 0 ? "text-muted" : ""}
            >
              <FinishedTasksScreen tasks={snapshot.finished} />
            </Tab>
            <Tab
              eventKey={ViewType.Preferences}
              title={
                <TabTitle
                  title="Preferences"
                  disabled={overdueTasks.length > 0}
                  tooltip={overdueTasks.length > 0 ? "Please resolve overdue tasks first" : undefined}
                />
              }
              tabClassName={overdueTasks.length > 0 ? "text-muted" : ""}
            >
              <PreferencesScreen state={state} setState={setState} />
            </Tab>
          </Tabs>
        </Stack>
      </Col>
    </Row>
  );
};

const RestoredScreen: React.FC<RestoredScreenProps> = ({
  state,
  connectWebsocket
}) => {
  return (
    <Button
      variant="link"
      className="w-100 h-100"
      onClick={() => connectWebsocket(state.apiKey)}
    >
      Resume Session
    </Button>
  );
};

const NotConnectedScreen: React.FC<NotConnectedScreenProps> = ({
  state,
  expanded,
  expandDock,
  connectWebsocket
}) => {
  return (
    <Row className="g-2">
      <Col>
        <Button variant="link" className="w-100 h-100" onClick={expandDock}>
          {state.error ? (
            <span className="text-danger">{state.error}</span>
          ) : (
            "Connecting..."
          )}
        </Button>
      </Col>
      {state.error && expanded && (
        <Col xs="auto">
          <Button
            variant="secondary"
            onClick={() => connectWebsocket(state.apiKey)}
          >
            Retry
          </Button>
        </Col>
      )}
    </Row>
  );
};

function App() {
  // Main app state
  const [state, setState] = useState<AppState>({
    type: "NotLoggedIn"
  });

  // UI state
  const [expanded, setExpanded_raw] = useState(false);
  const [focused, setFocused] = useState(false);
  const [windowFocused, setWindowFocused_raw] = useState(false);

  // Add effect to check for overdue tasks
  useEffect(() => {
    if (state.type !== "Connected") return;

    const connectedState = state as Extract<AppState, { type: "Connected" }>;
    // Check if any live tasks are overdue
    const hasOverdueTasks = connectedState.snapshot.live.some(task => {
      if (!task.deadline) return false;
      return (Date.now() / 1000) > task.deadline;
    });

    // If there are overdue tasks, expand the window and switch to overdue tab
    if (hasOverdueTasks) {
      if (!expanded) {
        expandDock();
      }
      // Only update view type if we're not already on the overdue tab
      if (connectedState.viewType !== ViewType.Overdue) {
        setState({
          ...connectedState,
          viewType: ViewType.Overdue
        });
      }
    }
  }, [state.type === "Connected" ? (state as Extract<AppState, { type: "Connected" }>).snapshot.live : [], expanded]); // Only depend on live tasks and expanded state

  // Add periodic check for overdue tasks
  useEffect(() => {
    if (state.type !== "Connected") return;

    const connectedState = state as Extract<AppState, { type: "Connected" }>;
    // Check every second for overdue tasks
    const interval = setInterval(() => {
      const hasOverdueTasks = connectedState.snapshot.live.some(task => {
        if (!task.deadline) return false;
        return (Date.now() / 1000) > task.deadline;
      });

      if (hasOverdueTasks) {
        if (!expanded) {
          expandDock();
        }
        // Only update view type if we're not already on the overdue tab
        if (connectedState.viewType !== ViewType.Overdue) {
          setState({
            ...connectedState,
            viewType: ViewType.Overdue
          });
        }
      }
    }, 1000);

    return () => clearInterval(interval);
  }, [state.type, expanded, state.type === "Connected" ? (state as Extract<AppState, { type: "Connected" }>).snapshot.live : [], state.type === "Connected" ? (state as Extract<AppState, { type: "Connected" }>).viewType : ViewType.Live]); // Include all dependencies used in the interval

  // Save preferences to cache when they change
  useEffect(() => {
    if (state.type !== "Connected") return;

    const cache = loadCache();
    if (cache) {
      saveCache({
        ...cache,
        preferences: state.preferences
      });
    }
  }, [state.type === "Connected" ? state.preferences : undefined]);

  // Vocal reminders
  useEffect(() => {
    if (state.type !== "Connected") return;

    const connectedState = state as Extract<AppState, { type: "Connected" }>;

    if (!connectedState.preferences.vocalEnabled) return;

    const speakTopTask = async () => {
      const topTask = connectedState.snapshot.live[0];
      if (topTask) {
        // Debounce: only speak if at least 5 seconds have passed since last message
        const now = Date.now();
        const timeSinceLastSpeak = now - lastVocalMessageTimeRef.current;

        if (timeSinceLastSpeak < 5000) {
          console.log("Skipping vocal reminder (debounced)");
          return;
        }

        try {
          await invoke("speak_message", { message: topTask.value });
          lastVocalMessageTimeRef.current = now;
        } catch (error) {
          console.error("Failed to speak task:", error);
        }
      }
    };

    // Speak immediately when enabled
    speakTopTask();

    // Set up interval for periodic reminders
    const interval = setInterval(speakTopTask, connectedState.preferences.vocalFrequency * 1000);

    return () => clearInterval(interval);
  }, [
    state.type === "Connected" ? state.preferences.vocalEnabled : false,
    state.type === "Connected" ? state.preferences.vocalFrequency : 0,
    state.type === "Connected" ? state.snapshot.live : []
  ]);

  // Default server URL
  const defaultServerUrl = "http://localhost:8080/public/";
  const [serverApiUrl, setServerApiUrl] = useState("");

  // Refs for inputs
  const emailInputRef = useRef<HTMLInputElement>(null);
  const passwordInputRef = useRef<HTMLInputElement>(null);
  const taskInputRef = useRef<HTMLInputElement>(null);
  const activeTaskInputRef = useRef<HTMLInputElement>(null);

  // Ref to track last vocal message time (for debouncing)
  const lastVocalMessageTimeRef = useRef<number>(0);

  // Create the new setWindowFocused function that handles the Tauri invoke
  const setWindowFocused = async (newState: boolean) => {
    try {
      console.debug(`Attempting to set window focus state to: ${newState}`);
      await invoke('set_focus_state', { focused: newState });
    } catch (error) {
      console.error('Failed to change window focus state:', error);
    }
    setWindowFocused_raw(newState);

  };

  // Create the new setExpand function that handles the Tauri invoke
  const setExpand = async (newState: boolean) => {
    try {
      console.debug(`Attempting to set window expand state to: ${newState}`);
      await invoke('set_expand_state', { expanded: newState });
    } catch (error) {
      console.error('Failed to change window size:', error);
    }
    setExpanded_raw(newState);
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
        activeIdVal: undefined,
        viewType: ViewType.Live
      });
    }
  };

  // Handle login attempt
  const attemptLogin = async (email: string, password: string) => {
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
          email: email,
          password: password,
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

      // Save to cache with default preferences, using the actual URL that was used
      saveCache({
        serverApiUrl: serverApiUrl,
        apiKey: apiKeyData.key,
        preferences: {
          vocalEnabled: false,
          vocalFrequency: 300 // 5 minutes in seconds
        }
      });

      // Update the serverApiUrl state to the actual URL used
      setServerApiUrl(serverApiUrl);

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

        // Load preferences from cache or use defaults
        const cache = loadCache();
        const preferences = cache?.preferences || {
          vocalEnabled: false,
          vocalFrequency: 300 // 5 minutes in seconds
        };

        setState({
          type: "Connected",
          apiKey,
          inputValue: "",
          activeIdVal: undefined,
          snapshot: {
            live: [],
            finished: []
          },
          viewType: ViewType.Live,
          sessionId,
          preferences,
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
            console.error('WebSocket message validation error:', error.issues);
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
    setState({ type: "NotLoggedIn" });
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
        setState(prevState => ({
          ...prevState,
          viewType: state.viewType === ViewType.Finished ? ViewType.Live : ViewType.Finished,
          inputValue: "",
          activeIdVal: undefined
        }));
        return;

      case "s": // succeed first task
        if (state.snapshot.live.length > 0) {
          const task = state.snapshot.live[0];
          finishTask(task.id, "Succeeded");
          setState(prevState => ({
            ...prevState,
            inputValue: "",
            activeIdVal: undefined
          }));
        }
        return;

      case "f": // fail first task
        if (state.snapshot.live.length > 0) {
          const task = state.snapshot.live[0];
          finishTask(task.id, "Failed");
          setState(prevState => ({
            ...prevState,
            inputValue: "",
            activeIdVal: undefined
          }));
        }
        return;

      case "o": // obsolete first task
        if (state.snapshot.live.length > 0) {
          const task = state.snapshot.live[0];
          finishTask(task.id, "Obsoleted");
          setState(prevState => ({
            ...prevState,
            inputValue: "",
            activeIdVal: undefined
          }));
        }
        return;

      case "r": // restore finished task
        const restoreIndex = parseRestoreCommand(inputValue);
        if (restoreIndex !== null && restoreIndex < state.snapshot.finished.length) {
          restoreFinishedTask(state.snapshot.finished[restoreIndex].id);
          setState(prevState => ({
            ...prevState,
            inputValue: "",
            activeIdVal: undefined
          }));
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
          setState(prevState => ({
            ...prevState,
            inputValue: "",
            activeIdVal: undefined
          }));
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
            setState(prevState => ({
              ...prevState,
              inputValue: "",
              activeIdVal: undefined
            }));
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
            setState(prevState => ({
              ...prevState,
              inputValue: "",
              activeIdVal: undefined
            }));
          }
        }
        return;

      case "d": // set due date for first task
        const newDeadline = parseDueCommand(inputValue);
        if (newDeadline !== null && state.snapshot.live.length > 0) {
          const task = state.snapshot.live[0];
          editTask(task.id, task.value, newDeadline);
          setState(prevState => ({
            ...prevState,
            inputValue: "",
            activeIdVal: undefined
          }));
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
  const addNewTask = (value: string, deadline: number | null = null) => {
    if (state.type !== "Connected") return;

    const taskId = randomString();

    // Only clear the input if the send was successful
    if (sendWsOp({
      InsLiveTask: {
        id: taskId,
        value,
        deadline
      }
    })) {
      setState(prevState => ({
        ...prevState,
        inputValue: "",
      }));
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

  const editTask = (id: string, value: string, deadline: number | null) => {
    if (state.type !== "Connected") return;

    sendWsOp({
      EditLiveTask: {
        id,
        value,
        deadline
      }
    });
  };

  const setActiveTask = (id?: string) => {
    if (state.type !== "Connected") return;

    // If there was an active task being edited, save it first
    if (state.activeIdVal) {
      const [activeId, activeValue, activeDeadline] = state.activeIdVal;
      // Send the changes to the server
      editTask(activeId, activeValue, activeDeadline);
    }

    if (!id) {
      // Just update UI state, no server interaction needed here
      setState(prevState => ({
        ...prevState,
        activeIdVal: undefined
      }));
      setTimeout(() => {
        taskInputRef.current?.focus();
      }, 0);
      return;
    }

    // Find the task
    const task = state.snapshot.live.find(task => task.id === id);
    if (!task) return;

    // Just update local editing state, no server interaction needed here
    setState(prevState => ({
      ...prevState,
      activeIdVal: [task.id, task.value, task.deadline]
    }));

    setTimeout(() => {
      activeTaskInputRef.current?.focus();
    }, 0);
  };

  const renderContent = () => {
    if (state.type === "NotLoggedIn") {
      return (
        <LoginScreen
          state={state}
          expanded={expanded}
          emailInputRef={emailInputRef}
          passwordInputRef={passwordInputRef}
          expandDock={expandDock}
          collapseDock={collapseDock}
          setState={setState}
          attemptLogin={attemptLogin}
          serverApiUrl={serverApiUrl}
          setServerApiUrl={setServerApiUrl}
        />
      );
    }

    if (state.type === "Connected") {
      return (
        <ConnectedScreen
          state={state}
          expanded={expanded}
          taskInputRef={taskInputRef}
          activeTaskInputRef={activeTaskInputRef}
          expandDock={expandDock}
          collapseDock={collapseDock}
          setState={setState}
          logout={logout}
          submitTask={submitTask}
          finishTask={finishTask}
          setActiveTask={setActiveTask}
          editTask={editTask}
        />
      );
    }

    if (state.type === "Restored") {
      return (
        <RestoredScreen
          state={state}
          connectWebsocket={connectWebsocket}
        />
      );
    }

    if (state.type === "NotConnected") {
      return (
        <NotConnectedScreen
          state={state}
          expanded={expanded}
          expandDock={expandDock}
          connectWebsocket={connectWebsocket}
        />
      );
    }
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
      {renderContent()}
    </Container>
  );
}

export default App;