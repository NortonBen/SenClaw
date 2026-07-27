import axios from 'axios';

// Single-user local app: same-origin API, no auth headers.
const api = axios.create({
  baseURL: '/api',
  headers: {
    'Content-Type': 'application/json',
  },
});

export default api;
