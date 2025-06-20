// src/App.jsx
import { useState, useEffect, useMemo } from 'react';

// Helper for API calls
const API_URL = 'https://tasktrack-4jn7.onrender.com';
async function fetchTasks() {
  const resp = await fetch(`${API_URL}/notes/`);
  if (!resp.ok) throw new Error('Failed to fetch tasks');
  return resp.json();
}

function Login({ onLogin }) {
  const [user, setUser] = useState('');
  const [pass, setPass] = useState('');
  const submit = (e) => {
    e.preventDefault();
    if (user === 'admin' && pass === 'admin') onLogin();
    else alert('Invalid credentials');
  };
  return (
    <div className="flex items-center justify-center h-screen bg-gray-100">
      <form onSubmit={submit} className="bg-white p-6 rounded shadow-md w-80">
        <h2 className="text-xl font-bold mb-4">Login</h2>
        <input type="text" placeholder="Username" className="w-full mb-2 p-2 border rounded"
          value={user} onChange={e => setUser(e.target.value)} />
        <input type="password" placeholder="Password" className="w-full mb-4 p-2 border rounded"
          value={pass} onChange={e => setPass(e.target.value)} />
        <button type="submit" className="bg-blue-500 text-white px-4 py-2 rounded w-full">Login</button>
      </form>
    </div>
  );
}

async function createTask() {
  const resp = await fetch(`${API_URL}/notes/`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ title: 'New Task', content: 'asd', tags: ['xyz'] })
  });
  // console.error('Error creating task:', resp.status, errorBody);
  if (!resp.ok) {
    const errorBody = await resp.json().catch(() => null);
    console.error('Error creating task:', resp.status, errorBody);
    throw new Error(`Failed to create task: ${resp.status} ${JSON.stringify(errorBody)}`);
  }

  return resp.json();
}

function formatDuration(seconds) {
  const h = String(Math.floor(seconds / 3600)).padStart(2, '0');
  const m = String(Math.floor((seconds % 3600) / 60)).padStart(2, '0');
  const s = String(seconds % 60).padStart(2, '0');
  return `${h}:${m}:${s}`;
}

async function updateTask(task) {
  const resp = await fetch(`${API_URL}/notes/${task.id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      title: task.title,
      content: task.content,
      tags: task.tags || []
    })
  });
  return resp.json();
}

function MainApp() {
  const [tasks, setTasks] = useState([]);
  const [selectedTask, setSelectedTask] = useState(null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    fetchTasks().then(setTasks).catch(console.error);
  }, []);

  useEffect(() => {
    const interval = setInterval(() => setTick(t => t + 1), 1000);
    return () => clearInterval(interval);
  }, []);

  const handleAdd = async () => {
    const newTask = await createTask();
    setTasks(prev => [...prev, newTask]);
    setSelectedTask(newTask);
  };

  const handleSelect = async (taskId) => {
    const resp = await fetch(`${API_URL}/notes/${taskId}`);
    const full = await resp.json();
    setSelectedTask(full);
  };

  const handleDelete = async (id) => {
    await fetch(`${API_URL}/notes/${id}`, { method: 'DELETE' });
    setTasks(prev => prev.filter(t => t.id !== id));
    if (selectedTask?.id === id) setSelectedTask(null);
  };

  const handleContentChange = (e) => {
    setSelectedTask({ ...selectedTask, content: e.target.value });
  };

  const handleContentBlur = async () => {
    if (selectedTask) {
      const updated = await updateTask(selectedTask);
      setTasks(prev =>
        prev.map(t => (t.id === updated.id ? updated : t))
      );
      setSelectedTask(updated);
    }
  };

  const startTimer = async (id) => {
    await fetch(`${API_URL}/notes/${id}/start`, { method: 'POST' });
    const updated = await fetch(`${API_URL}/notes/${id}`).then(res => res.json());
    setSelectedTask(updated);
    setTasks(prev => prev.map(t => t.id === updated.id ? updated : t));
  };

  const stopTimer = async (id) => {
    await fetch(`${API_URL}/notes/${id}/stop`, { method: 'POST' });
    const updated = await fetch(`${API_URL}/notes/${id}`).then(res => res.json());
    setSelectedTask(updated);
    setTasks(prev => prev.map(t => t.id === updated.id ? updated : t));
  };

  const displayedDuration = useMemo(() => {
    if (!selectedTask) return 0;
    const base = selectedTask.elapsed_seconds || 0;
    const delta = selectedTask.started_at
      ? (Math.floor((Date.now() - Date.parse(selectedTask.started_at)) / 1000))
      : 0;
    console.log("delta " + delta);
    return base + delta;
  }, [tick, selectedTask]);

  return (
    <div className="grid grid-cols-4 grid-rows-10 gap-2 h-screen p-2 bg-gray-200">
      {/* Task List Panel */}
      <div className="bg-white rounded shadow p-2 col-span-1 row-span-9 overflow-auto">
        <h3 className="font-bold mb-2">Tasks</h3>
        {tasks.map(t => (
          <div
            key={t.id}
            className={`flex justify-between items-center cursor-pointer px-2 py-1 rounded ${
              selectedTask?.id === t.id ? 'bg-blue-100' : 'hover:bg-gray-100'
            }`}
            onClick={() => handleSelect(t.id)}
          >
            <span className="truncate">{t.title}</span>
            <button
              className="text-red-500 text-xs ml-2 hover:text-red-700"
              onClick={(e) => {
                e.stopPropagation();
                handleDelete(t.id);
              }}
            >
              ✖
            </button>
          </div>
        ))}
      </div>

      {/* Task Meta Panel */}
      <div className="bg-white rounded shadow p-2 col-span-3 row-span-1 flex items-center">
        {selectedTask ? (
          <div className="flex flex-col w-full">
            <input
              className="text-2xl font-bold w-full border-b border-gray-300 focus:outline-none mb-2"
              value={selectedTask.title}
              onChange={(e) =>
                setSelectedTask(prev => ({ ...prev, title: e.target.value }))
              }
              onBlur={async () => {
                const updated = await updateTask(selectedTask);
                setTasks(prev =>
                  prev.map(t => (t.id === updated.id ? updated : t))
                );
                setSelectedTask(updated);
              }}
            />
            <div className="flex items-center gap-4 text-sm">
              <button
                onClick={() => startTimer(selectedTask.id)}
                className="px-3 py-1 bg-blue-500 text-white rounded"
              >
                Start
              </button>
              <button
                onClick={() => stopTimer(selectedTask.id)}
                className="px-3 py-1 bg-gray-500 text-white rounded"
              >
                Stop
              </button>
              <span className="text-gray-600 font-mono">
                {formatDuration(displayedDuration)}
              </span>
            </div>
          </div>
        ) : (
          <h3 className="font-bold">Task Meta Info</h3>
        )}
      </div>

      {/* Note Content Panel */}
      <div className="bg-white rounded shadow p-2 col-span-3 row-span-9 overflow-auto">
        <textarea
          className="border rounded p-2 mt-2 w-full h-full"
          value={selectedTask?.content || ''}
          onChange={handleContentChange}
          onBlur={handleContentBlur}
          placeholder="Select a task to edit notes"
        />
      </div>

      {/* Add Task Button */}
      <div className="bg-white rounded shadow p-2 col-span-1 row-span-1 flex items-center justify-center order-last">
        <button
          onClick={handleAdd}
          className="bg-green-500 text-white px-4 py-2 rounded"
        >
          Add Task
        </button>
      </div>
    </div>
  );
}

export default MainApp;
// export default function App() {
//   const [loggedIn, setLoggedIn] = useState(false);
//   return loggedIn ? <MainApp /> : <Login onLogin={() => setLoggedIn(true)} />;
// }
