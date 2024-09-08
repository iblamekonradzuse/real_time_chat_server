        function showRegisterForm() {
            document.getElementById('loginForm').style.display = 'none';
            document.getElementById('registerForm').style.display = 'block';
        }

        function showLoginForm() {
            document.getElementById('registerForm').style.display = 'none';
            document.getElementById('loginForm').style.display = 'block';
        }

        function toggleDarkMode() {
            document.body.classList.toggle('dark-mode');
        }

        // WebSocket connection
        let socket;

        function connectWebSocket(username) {
            socket = new WebSocket(`ws://${window.location.host}/chat?username=${encodeURIComponent(username)}&password=${encodeURIComponent(document.getElementById('loginPassword').value)}`);

            socket.onmessage = function(event) {
                const chat = document.getElementById('chat');
                chat.innerHTML += event.data + '<br>';
                chat.scrollTop = chat.scrollHeight;
            };

            socket.onclose = function() {
                console.log('WebSocket connection closed');
            };
        }

        function login() {
            const username = document.getElementById('loginUsername').value;
            const password = document.getElementById('loginPassword').value;

            fetch('/login', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ username, password }),
            })
            .then(response => response.json())
            .then(data => {
                if (data.status === 'success') {
                    document.getElementById('loginForm').style.display = 'none';
                    document.getElementById('chatForm').style.display = 'block';
                    connectWebSocket(username);
                } else {
                    alert(data.message);
                }
            })
            .catch(error => {
                console.error('Error:', error);
                alert('An error occurred during login');
            });
        }

        function register() {
            const username = document.getElementById('registerUsername').value;
            const password = document.getElementById('registerPassword').value;

            fetch('/register', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ username, password }),
            })
            .then(response => response.json())
            .then(data => {
                if (data.status === 'success') {
                    alert('Registration successful. Please login.');
                    showLoginForm();
                } else {
                    alert(data.message);
                }
            })
            .catch(error => {
                console.error('Error:', error);
                alert('An error occurred during registration');
            });
        }

        document.getElementById('messageForm').addEventListener('submit', function(e) {
            e.preventDefault();
            const messageInput = document.getElementById('message');
            const message = messageInput.value;
            if (message && socket.readyState === WebSocket.OPEN) {
                socket.send(message);
                messageInput.value = '';
            }
        });

