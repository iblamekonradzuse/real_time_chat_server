let socket;
let currentUser;

function showRegisterForm() {
    document.getElementById('loginForm').style.display = 'none';
    document.getElementById('registerForm').style.display = 'block';
}

function showLoginForm() {
    document.getElementById('registerForm').style.display = 'none';
    document.getElementById('loginForm').style.display = 'block';
}

function connectWebSocket(username) {
    currentUser = username;
    socket = new WebSocket(`ws://${window.location.host}/chat?username=${encodeURIComponent(username)}&password=${encodeURIComponent(document.getElementById('loginPassword').value)}`);

    socket.onmessage = function(event) {
        const data = JSON.parse(event.data);
        const chat = document.getElementById('chat');
        
        if (data.type === 'message') {
            const messageElement = createMessageElement(data);
            chat.appendChild(messageElement);
        } else if (data.type === 'edit') {
            const messageElement = document.getElementById(`message-${data.id}`);
            if (messageElement) {
                messageElement.querySelector('.content').textContent = data.content;
            }
        } else if (data.type === 'delete') {
            const messageElement = document.getElementById(`message-${data.id}`);
            if (messageElement) {
                messageElement.remove();
            }
        }
        
        chat.scrollTop = chat.scrollHeight;
    };

    socket.onclose = function() {
        console.log('WebSocket connection closed');
    };
}

function createMessageElement(data) {
    const messageElement = document.createElement('div');
    messageElement.className = 'message';
    messageElement.id = `message-${data.id}`;

    const usernameElement = document.createElement('span');
    usernameElement.className = 'username';
    usernameElement.textContent = data.username;

    const contentElement = document.createElement('span');
    contentElement.className = 'content';
    contentElement.textContent = data.content;

    messageElement.appendChild(usernameElement);
    messageElement.appendChild(contentElement);

    if (data.username === currentUser) {
        const actionsElement = document.createElement('span');
        actionsElement.className = 'actions';

        const editButton = document.createElement('button');
        editButton.innerHTML = '&#9998;'; // Pencil icon
        editButton.className = 'icon-button edit';
        editButton.title = 'Edit';
        editButton.onclick = () => editMessage(data.id);

        const deleteButton = document.createElement('button');
        deleteButton.innerHTML = '&#128465;'; // Trash bin icon
        deleteButton.className = 'icon-button delete';
        deleteButton.title = 'Delete';
        deleteButton.onclick = () => deleteMessage(data.id);

        actionsElement.appendChild(editButton);
        actionsElement.appendChild(deleteButton);
        messageElement.appendChild(actionsElement);
    }

    return messageElement;
}

function editMessage(messageId) {
    const messageElement = document.getElementById(`message-${messageId}`);
    const contentElement = messageElement.querySelector('.content');
    const currentContent = contentElement.textContent;

    const inputElement = document.createElement('input');
    inputElement.type = 'text';
    inputElement.value = currentContent;
    inputElement.className = 'edit-input';

    const saveButton = document.createElement('button');
    saveButton.innerHTML = '&#10004;'; // Checkmark icon
    saveButton.className = 'icon-button save';
    saveButton.title = 'Save';
    saveButton.onclick = () => {
        const newContent = inputElement.value;
        socket.send(JSON.stringify({
            type: 'edit',
            id: messageId,
            content: newContent
        }));
        contentElement.textContent = newContent;
        messageElement.replaceChild(contentElement, inputElement);
        messageElement.querySelector('.actions').replaceChild(
            messageElement.querySelector('.edit'),
            saveButton
        );
    };

    messageElement.replaceChild(inputElement, contentElement);
    messageElement.querySelector('.actions').replaceChild(
        saveButton,
        messageElement.querySelector('.edit')
    );
}

function deleteMessage(messageId) {
    if (confirm('Are you sure you want to delete this message?')) {
        socket.send(JSON.stringify({
            type: 'delete',
            id: messageId
        }));
    }
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
    .then(response => {
        if (!response.ok) {
            return response.json().then(err => { throw err; });
        }
        return response.json();
    })
    .then(data => {
        if (data.status === 'success') {
            document.getElementById('loginForm').style.display = 'none';
            document.getElementById('chatForm').style.display = 'block';
            connectWebSocket(username);
        } else {
            alert(data.message || 'Login failed');
        }
    })
    .catch(error => {
        console.error('Error:', error);
        alert(error.message || 'An error occurred during login');
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
        body: JSON.stringify({ username, password_hash: password }),
    })
    .then(response => response.json())
    .then(data => {
        if (data.status === 'success') {
            alert('Registration successful. Please login.');
            showLoginForm();
        } else {
            alert(data.message || 'Registration failed');
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
        const messageObject = {
            type: 'message',
            content: message
        };
        console.log('Sending message:', JSON.stringify(messageObject));
        socket.send(JSON.stringify(messageObject));
        messageInput.value = '';
    }
});

