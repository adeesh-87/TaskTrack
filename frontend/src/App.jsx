// src/App.jsx
import { useState, useEffect } from 'react';

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

  useEffect(() => {
    fetchTasks()
      .then(setTasks)
      .catch(console.error);
  }, []);

  const handleAdd = async () => {
    const newTask = await createTask();
    setTasks(prev => [...prev, newTask]); // append to UI
    setSelectedTask(newTask);
  };

  const handleSelect = async (taskId) => {
    const resp = await fetch(`${API_URL}/notes/${taskId}`);
    const full = await resp.json();
    setSelectedTask(full);
  };

  const handleContentChange = async (e) => {
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

  return (
    <div className="grid grid-cols-4 grid-rows-10 gap-2 h-screen p-2 bg-gray-200">
      <div className="bg-white rounded shadow p-2 col-span-1 row-span-9 overflow-auto">
        <h3 className="font-bold mb-2">Tasks</h3>
        {tasks.map(t => (
          <div key={t.id}
            className={`cursor-pointer py-1 ${selectedTask?.id === t.id ? 'bg-blue-100' : ''}`}
            onClick={() => handleSelect(t.id)}>
            {t.title}
          </div>
        ))}
      </div>
      {/* Panel B: Task Title (editable) */}
      <div className="bg-white rounded shadow p-2 col-span-3 row-span-1 flex items-center">
        {selectedTask ? (
          <input
            className="text-2xl font-bold w-full border-b border-gray-300 focus:outline-none"
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
        ) : (
          <h3 className="font-bold">Task Meta Info</h3>
        )}
      </div>
      <div className="bg-white rounded shadow p-2 col-span-3 row-span-9 overflow-auto">
        <textarea
          className="border rounded p-2 mt-2 w-full h-full"
          value={selectedTask?.content || ''}
          onChange={handleContentChange}
          onBlur={handleContentBlur}
          placeholder="Select a task to edit notes"
        />
      </div>
      <div className="bg-white rounded shadow p-2 col-span-1 row-span-1 flex items-center justify-center order-last">
        <button onClick={handleAdd}
          className="bg-green-500 text-white px-4 py-2 rounded">Add Task</button>
      </div>
    </div>
  );
}

export default MainApp;
// export default function App() {
//   const [loggedIn, setLoggedIn] = useState(false);
//   return loggedIn ? <MainApp /> : <Login onLogin={() => setLoggedIn(true)} />;
// }
